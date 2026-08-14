//! Process orchestration for the live two-validator stack.
//!
//! Every validator can either be **spawned** by the test or **attached to**
//! when one is already running — see [`StackConfig::from_env`]. Attaching keeps
//! the iteration loop fast: leave a base validator up all day and re-run the
//! tests against it in a couple of seconds.

use std::{
    fs,
    io::ErrorKind,
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;

/// Default base-layer JSON-RPC port. The validator serves PubSub on `PORT + 1`.
pub const DEFAULT_BASE_RPC_PORT: u16 = 7101;
/// Default ephemeral-rollup JSON-RPC port. WS is served on `PORT + 1`.
pub const DEFAULT_ER_RPC_PORT: u16 = 7799;

/// Which parts of the stack this run owns versus attaches to.
#[derive(Clone, Copy, Debug)]
pub struct StackConfig {
    pub base_rpc_port: u16,
    pub er_rpc_port: u16,
    /// When false, a base validator is expected to already be listening.
    pub spawn_base: bool,
    /// When false, an ephemeral validator is expected to already be listening.
    pub spawn_er: bool,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            base_rpc_port: DEFAULT_BASE_RPC_PORT,
            er_rpc_port: DEFAULT_ER_RPC_PORT,
            spawn_base: true,
            spawn_er: true,
        }
    }
}

impl StackConfig {
    /// Read the configuration from the environment:
    ///
    /// | variable                  | effect                                      |
    /// |---------------------------|---------------------------------------------|
    /// | `E2E_SKIP_BASE_VALIDATOR` | attach to a running base instead of spawning |
    /// | `E2E_SKIP_ER_VALIDATOR`   | attach to a running rollup instead of spawning |
    /// | `E2E_BASE_RPC_PORT`       | base JSON-RPC port (default 7101)            |
    /// | `E2E_ER_RPC_PORT`         | rollup JSON-RPC port (default 7799)          |
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            base_rpc_port: env_port("E2E_BASE_RPC_PORT", default.base_rpc_port),
            er_rpc_port: env_port("E2E_ER_RPC_PORT", default.er_rpc_port),
            spawn_base: !env_flag("E2E_SKIP_BASE_VALIDATOR"),
            spawn_er: !env_flag("E2E_SKIP_ER_VALIDATOR"),
        }
    }

    pub fn base_rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.base_rpc_port)
    }

    pub fn base_ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.base_rpc_port + 1)
    }

    pub fn er_rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.er_rpc_port)
    }
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => !matches!(v.trim(), "" | "0" | "false" | "no"),
        Err(_) => false,
    }
}

fn env_port(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// True when something is already listening on `127.0.0.1:port`.
pub fn port_in_use(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().expect("valid loopback addr"),
        Duration::from_millis(250),
    )
    .is_ok()
}

/// Owns spawned child processes and tears them down on drop — including on
/// panic, so a failing assertion never leaks a validator. Attached validators
/// are not owned and are therefore left running.
pub struct Stack {
    children: Vec<(String, Child)>,
    tmp: TempDir,
    pub config: StackConfig,
}

impl Stack {
    /// Bring up (or attach to) the base validator and the ephemeral rollup.
    ///
    /// `base_programs` are `(program_id, path_to_so)` pairs loaded into the
    /// base validator at genesis; the rollup clones them on demand. They are
    /// ignored for an attached base validator, which must already have them.
    pub fn start(config: StackConfig, base_programs: &[(String, PathBuf)]) -> Result<Self> {
        let tmp = TempDir::new()?;
        let mut stack = Stack {
            children: Vec::new(),
            tmp,
            config,
        };
        stack.start_base(base_programs)?;
        stack.start_er()?;
        Ok(stack)
    }

    pub fn tmp_path(&self) -> &Path {
        self.tmp.path()
    }

    pub fn base_rpc(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.config.base_rpc_url(), CommitmentConfig::confirmed())
    }

    pub fn er_rpc(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.config.er_rpc_url(), CommitmentConfig::confirmed())
    }

    fn start_base(&mut self, base_programs: &[(String, PathBuf)]) -> Result<()> {
        let port = self.config.base_rpc_port;
        if !self.config.spawn_base {
            if !port_in_use(port) {
                bail!(
                    "E2E_SKIP_BASE_VALIDATOR is set but nothing is listening on 127.0.0.1:{port}. \
                     Start one with `make e2e-base-validator`, or unset the flag to spawn one."
                );
            }
            eprintln!("[stack] attaching to base validator on :{port}");
        } else {
            if port_in_use(port) {
                bail!(
                    "127.0.0.1:{port} is already in use. Either stop the process holding it, \
                     or re-run with E2E_SKIP_BASE_VALIDATOR=1 to reuse it."
                );
            }
            eprintln!("[stack] starting mb-test-validator on :{port}");
            let ledger = self.tmp.path().join("base-ledger");
            let mut args: Vec<String> = vec![
                "--reset".into(),
                "--quiet".into(),
                "--ledger".into(),
                ledger.to_string_lossy().into_owned(),
                "--rpc-port".into(),
                port.to_string(),
                "--bind-address".into(),
                "127.0.0.1".into(),
            ];
            for (program_id, so) in base_programs {
                args.push("--bpf-program".into());
                args.push(program_id.clone());
                args.push(so.to_string_lossy().into_owned());
            }
            let log = fs::File::create(self.tmp.path().join("base.log"))?;
            let child = spawn("mb-test-validator", &args, log, &[])?;
            self.children.push(("mb-test-validator".into(), child));
        }

        let rpc = self.base_rpc();
        wait_for_rpc(&rpc, "base", Duration::from_secs(30))
    }

    fn start_er(&mut self) -> Result<()> {
        let port = self.config.er_rpc_port;
        if !self.config.spawn_er {
            if !port_in_use(port) {
                bail!(
                    "E2E_SKIP_ER_VALIDATOR is set but nothing is listening on 127.0.0.1:{port}. \
                     Start one with `make e2e-er-validator`, or unset the flag to spawn one."
                );
            }
            eprintln!("[stack] attaching to ephemeral validator on :{port}");
        } else {
            if port_in_use(port) {
                bail!(
                    "127.0.0.1:{port} is already in use. Either stop the process holding it, \
                     or re-run with E2E_SKIP_ER_VALIDATOR=1 to reuse it."
                );
            }
            eprintln!("[stack] starting ephemeral-validator on :{port}");
            let storage = self.tmp.path().join("er-storage");
            let args: Vec<String> = vec![
                "--no-tui".into(),
                "--reset".into(),
                "--lifecycle".into(),
                "ephemeral".into(),
                "--remotes".into(),
                self.config.base_rpc_url(),
                "--remotes".into(),
                self.config.base_ws_url(),
                "--listen".into(),
                format!("127.0.0.1:{port}"),
                "--storage".into(),
                storage.to_string_lossy().into_owned(),
            ];
            let log = fs::File::create(self.tmp.path().join("er.log"))?;
            // `warn` keeps a passing run quiet; set E2E_ER_LOG=info (with
            // KEEP_E2E_LOGS=1) when a post-action or intent bundle misbehaves
            // and you need to see what the rollup did with it.
            let er_log = std::env::var("E2E_ER_LOG").unwrap_or_else(|_| "warn".to_string());
            let child = spawn("ephemeral-validator", &args, log, &[("RUST_LOG", &er_log)])?;
            self.children.push(("ephemeral-validator".into(), child));
        }

        let rpc = self.er_rpc();
        wait_for_rpc(&rpc, "rollup", Duration::from_secs(30))?;
        // Give the rollup's remote-account-provider WS pool a moment to warm up
        // before we make it clone accounts from the base.
        std::thread::sleep(Duration::from_secs(3));
        Ok(())
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // Reverse order: rollup first, then base.
        for (name, child) in self.children.iter_mut().rev() {
            terminate(name, child);
        }
    }
}

/// Terminate a child's whole process group. The validators are launched via npm
/// wrappers that re-spawn the real binary as a grandchild and forward only
/// SIGINT, so a plain `kill <wrapper>` would orphan the validator. Each child
/// leads its own process group, so a negative PID reaches the whole tree.
fn terminate(name: &str, child: &mut Child) {
    eprintln!("[stack] stopping {name}");
    let pid = child.id();
    let group = format!("-{pid}");
    let signal = |sig: &str| {
        let _ = Command::new("kill").arg(sig).arg(&group).stderr(Stdio::null()).status();
    };
    signal("-INT");
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => break,
        }
    }
    signal("-KILL");
    let _ = child.kill();
    let _ = child.wait();
}

/// Spawn a child process into its own process group, logging to `log`.
fn spawn(program: &str, args: &[String], log: fs::File, envs: &[(&str, &str)]) -> Result<Child> {
    use std::os::unix::process::CommandExt;
    let err_log = log.try_clone().context("clone log handle")?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log))
        .process_group(0);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.spawn().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            anyhow!(
                "`{program}` not found on PATH. Install it with \
                 `npm i -g @magicblock-labs/ephemeral-validator`."
            )
        } else {
            anyhow!("failed to spawn `{program}`: {e}")
        }
    })
}

/// Block until `rpc` reports a slot, or `timeout` elapses.
pub fn wait_for_rpc(rpc: &RpcClient, label: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut last_err = String::new();
    while Instant::now() < deadline {
        match rpc.get_slot() {
            Ok(slot) if slot > 0 => {
                eprintln!("[stack] {label} healthy at slot {slot}");
                return Ok(());
            }
            Ok(_) => {}
            Err(e) => last_err = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    bail!("{label} did not become healthy within {timeout:?}: {last_err}");
}

/// A self-deleting temp directory for validator ledgers, storage and logs.
/// Set `KEEP_E2E_LOGS` to retain it for post-mortem.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Result<Self> {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        p.push(format!("e-token-e2e-{nanos}"));
        fs::create_dir_all(&p).with_context(|| format!("create temp dir {}", p.display()))?;
        Ok(TempDir(p))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::env::var_os("KEEP_E2E_LOGS").is_some() {
            eprintln!("[stack] KEEP_E2E_LOGS set — leaving {}", self.0.display());
            return;
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}
