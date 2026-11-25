//! Transaction submission and account helpers shared by the e2e tests.

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use solana_client::rpc_client::RpcClient;
use solana_instruction::Instruction;
use solana_keypair::{Keypair, Signature};
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_transaction::Transaction;

/// Send + confirm a transaction with an explicit fee payer and signer set,
/// retrying transient submission failures.
///
/// Always `skip_preflight`: on the rollup, account cloning and delegation
/// adoption happen on the real send path, not during preflight simulation, so
/// a simulation would wrongly reject accounts it has not cloned yet. The real
/// outcome is read back by polling the signature.
pub fn send(rpc: &RpcClient, ixs: &[Instruction], fee_payer: &Pubkey, signers: &[&Keypair]) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(1500));
        }
        match submit(rpc, ixs, fee_payer, signers) {
            // Submission succeeded, so the transaction may execute. Never
            // re-sign it; only poll this signature.
            Ok(sig) => return confirm(rpc, &sig),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("send failed")))
}

fn submit(rpc: &RpcClient, ixs: &[Instruction], fee_payer: &Pubkey, signers: &[&Keypair]) -> Result<Signature> {
    let bh = rpc.get_latest_blockhash().context("latest_blockhash")?;
    let msg = Message::new(ixs, Some(fee_payer));
    let tx = Transaction::new(signers, msg, bh);
    rpc.send_transaction_with_config(
        &tx,
        RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        },
    )
    .context("send_transaction")
}

fn confirm(rpc: &RpcClient, sig: &Signature) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match rpc.get_signature_status(sig)? {
            Some(Ok(())) => return Ok(()),
            // The instruction index and error code are the whole diagnosis;
            // the validator's own log has the program output.
            Some(Err(e)) => bail!("tx {sig} reverted: {e:?}"),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(250)),
            None => bail!("tx {sig} not confirmed within 30s"),
        }
    }
}

pub fn airdrop(rpc: &RpcClient, who: &Pubkey, lamports: u64) -> Result<()> {
    let sig = rpc
        .request_airdrop(who, lamports)
        .with_context(|| format!("airdrop {lamports} lamports to {who}"))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if rpc.confirm_transaction(&sig).unwrap_or(false) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    bail!("airdrop to {who} not confirmed in time");
}

/// Poll `f` until it returns `Some`, or `timeout` elapses.
pub fn wait_for<T>(timeout: Duration, label: &str, mut f: impl FnMut() -> Option<T>) -> Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if Instant::now() >= deadline {
            bail!("timed out after {timeout:?} waiting for {label}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Fetch an account's data, returning `None` when it does not exist.
pub fn account_data(rpc: &RpcClient, address: &Pubkey) -> Option<Vec<u8>> {
    rpc.get_account_with_commitment(address, rpc.commitment())
        .ok()
        .and_then(|r| r.value)
        .map(|a| a.data)
}
