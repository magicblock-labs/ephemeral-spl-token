//! End-to-end test harness for the ephemeral SPL token program.
//!
//! These tests run against the **real** two-validator MagicBlock stack rather
//! than an in-process `solana-program-test` bank:
//!
//! ```text
//!   mb-test-validator   ── base L1 (delegation + magic programs preloaded),
//!                          e-token + acl + hydra-ephemeral loaded at genesis
//!         ▲  clones programs/accounts on demand
//!         │
//!   ephemeral-validator ── the rollup; hosts the delegated queue, and runs the
//!                          scheduler that actually fires the crank
//! ```
//!
//! That matters for the transfer-queue crank specifically. The program asks for
//! the crank through the magic program's `ScheduleTask`, and the in-process
//! `solana-program-test` suite can only observe that request through a mock that
//! captures it and never executes it. Whether a scheduled task is then really
//! fired, on cadence, forever, is entirely the rollup's side of the contract —
//! and the rollup is where that moved onto Hydra. Only a live rollup tests it.
//!
//! # Prerequisites
//!
//! `mb-test-validator` and `ephemeral-validator` on `PATH`:
//!
//! ```sh
//! npm i -g @magicblock-labs/ephemeral-validator
//! ```
//!
//! and the program built (`cargo build-sbf`). `make test-e2e` does both and
//! runs the suite.
//!
//! `make test-e2e-full` additionally needs a Hydra cranker already running
//! against the rollup — nothing here starts one — since it settles a queued
//! transfer rather than only watching ticks.
//!
//! # Reusing a running validator
//!
//! Spawning a fresh base validator dominates the runtime. Leave one up and
//! re-run against it:
//!
//! ```sh
//! make e2e-base-validator                  # in one terminal
//! make test-e2e SKIP_BASE_VALIDATOR=1      # in another, repeatedly
//! ```
//!
//! See [`stack::StackConfig::from_env`] for the full set of variables.

pub mod fixture;
pub mod rpc;
pub mod stack;

use std::{path::PathBuf, sync::Mutex};

use anyhow::{bail, Result};

/// The tests bind fixed local ports and spawn validators, so they must not run
/// concurrently. `cargo` runs a test binary's tests on multiple threads, so a
/// process-wide lock serializes them (recovering from a poisoned lock if one
/// test panics).
pub static STACK_LOCK: Mutex<()> = Mutex::new(());

/// Repository root (`e-token-e2e` → `..`).
pub fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

/// Resolve a build artifact relative to the workspace root.
pub fn artifact(rel: &str) -> Result<PathBuf> {
    let p = workspace_root().join(rel);
    if !p.exists() {
        bail!(
            "missing artifact {} — run `cargo build-sbf` (see this crate's docs)",
            p.display()
        );
    }
    Ok(p)
}

/// The programs the base validator loads at genesis. The rollup clones them on
/// demand, so they only need to exist on the base.
pub fn base_programs() -> Result<Vec<(String, PathBuf)>> {
    Ok(vec![
        (
            fixture::PROGRAM_ID.to_string(),
            artifact("target/deploy/ephemeral_token_program.so")?,
        ),
        (
            fixture::PERMISSION_PROGRAM_ID.to_string(),
            artifact("e-token/tests/fixtures/acl.so")?,
        ),
        // The rollup's task scheduler creates every crank through this program.
        // Without it, `EnsureTransferQueueCrank` still succeeds and the queue
        // header still gets a task id, but the scheduler's own transaction is
        // rejected ("This program may not be used for executing instructions")
        // and no crank account is ever created — so nothing ticks.
        (
            fixture::HYDRA_EPHEMERAL_PROGRAM_ID.to_string(),
            artifact("e-token/tests/fixtures/hydra_ephemeral.so")?,
        ),
    ])
}
