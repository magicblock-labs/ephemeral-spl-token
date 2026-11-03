//! The full private-transfer flow, end to end across both chains:
//!
//! ```text
//!   base    ix 25: deposit into the vault, delegate a shuttle carrying
//!           post-actions, destination encrypted to the validator
//!   rollup  post-action decrypts and runs DepositAndQueueTransfer → queued
//!   rollup  the crank fires the tick → pops it, emits an intent bundle
//!   base    ExecuteReadyQueuedTransfer settles: vault ATA → recipient ATA,
//!           created on demand and paid for out of the rent PDA
//! ```
//!
//! After the single base-layer transaction, nothing here drives anything: the
//! rollup queues it, cranks it, and settles it back.
//!
//! Run it with `make test-e2e`. It passes against either rollup:
//! `ephemeral-validator`, which fires the scheduled task itself, and
//! `magicblock-validator`, which routes it through Hydra and therefore needs a
//! cranker running and the queue's crank funded (see [`fund_queue_crank`]).
//!
//! **When it fails, suspect the deployed programs before the code.** Three
//! builds have to match this repo, and each fails in a way that reads like a
//! program bug:
//!
//! - **e-token older than `target/deploy`** — the rollup rejects the whole
//!   post-action bundle with `Immutable`, because the older build marks accounts
//!   writable that this one does not. A bare `mb-test-validator` serves the npm
//!   package's own e-token build, so only `make e2e-base-validator` (or a
//!   suite-spawned validator) deploys this one.
//! - **`tests/fixtures/acl.so` older than ephemeral permissions** — the
//!   receipt's ACL CPI fails with `BorshIoError` after the transfer has already
//!   been queued, and the rollback hides that it got that far.
//! - **`hydra_ephemeral.so` missing from the base** — the crank is scheduled but
//!   never created, so the queued transfer is never settled.
//!
//! A stale validator is the likeliest cause of all three: `--bpf-program` is
//! read at genesis, so a validator started before the last `cargo build-sbf`
//! keeps serving the older program no matter how often the test is re-run.

use std::time::Duration;

use anyhow::{Context, Result};
use ephemeral_rollups_sdk::{
    consts::{EPHEMERAL_VAULT_ID, MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID, TOKEN_PROGRAM_ID},
    dlp_api::pda::magic_fee_vault_pda_from_validator,
    spl::{
        builders::{DelegateEphemeralAtaBuilder, EnsureTransferQueueCrankBuilder},
        find_vault_ata,
    },
};
use ephemeral_spl_api::{instruction::ESplInstruction, instructions::DepositAndQueueTransferArgs};
use ephemeral_token_e2e::{
    base_programs,
    fixture::{
        derive_group_receipt, fund_queue_crank, private_queued_transfer_ix, queue_header, setup_queue, token_balance,
        PROGRAM_ID,
    },
    rpc::{account_data, send, wait_for},
    stack::{Stack, StackConfig},
    STACK_LOCK,
};
use solana_client::rpc_client::RpcClient;
use solana_instruction::{AccountMeta, Instruction};
use solana_signer::Signer;
use wheels::layout::Encodable as _;

const DECIMALS: u8 = 6;

/// The flow crosses to the rollup and back through an intent bundle, so it
/// needs more slack than a plain tick.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Any shuttle id works; it only has to be unique per (owner, mint).
const SHUTTLE_ID: u32 = 1;

/// Bring the stack up, run `body`, and always tear down before asserting so a
/// panic never leaks validators.
fn with_stack<T>(body: impl FnOnce(&RpcClient, &RpcClient) -> Result<T>) -> T {
    let _guard = STACK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let programs = base_programs().expect("build artifacts present");
    let stack = Stack::start(StackConfig::from_env(), &programs).expect("stack starts");
    let result = body(&stack.base_rpc(), &stack.er_rpc());
    drop(stack);
    result.expect("e2e private transfer flow")
}

#[test]
#[ignore = "spawns live validators; run with `make test-e2e`"]
fn private_transfer_is_queued_cranked_and_settled_end_to_end() {
    with_stack(|base, er| {
        let fx = setup_queue(base, er, DECIMALS)?;
        let amount = 10 * 10u64.pow(DECIMALS as u32);

        // Not empty: the sender's eATA deposit is already escrowed in the vault,
        // which backs rollup-side balances too. What matters is the *delta*.
        let vault_before = token_balance(base, &fx.vault_ata)?;
        let sender_before = token_balance(base, &fx.sender_ata)?;
        assert!(
            account_data(base, &fx.recipient_ata).is_none(),
            "recipient ATA does not exist yet — settlement has to create it"
        );

        // `EnsureTransferQueueCrank` schedules the recurring queue crank through
        // the magic program's `ScheduleTask`. Sent to the rollup, which is what
        // turns that request into an actual recurring execution (Hydra backs the
        // rollup's task scheduler, but that is the rollup's implementation
        // detail — the program, and this test, only speak the magic crank
        // interface).
        send(
            er,
            &[EnsureTransferQueueCrankBuilder {
                payer: fx.payer.pubkey(),
                queue: fx.queue,
                magic_fee_vault: magic_fee_vault_pda_from_validator(&fx.validator),
                magic_context: MAGIC_CONTEXT_ID,
                magic_program: MAGIC_PROGRAM_ID,
            }
            .instruction()],
            &fx.payer.pubkey(),
            &[&fx.payer],
        )
        .context("EnsureTransferQueueCrank")?;
        // No-op on a rollup that fires its own tasks; required on one that
        // routes them through Hydra. See [`fund_queue_crank`].
        fund_queue_crank(er, &fx.payer, &fx.queue)?;

        let transfer = private_queued_transfer_ix(
            &fx.helper.pubkey(),
            &fx.payer.pubkey(),
            &fx.mint,
            &fx.validator,
            &fx.recipient,
            SHUTTLE_ID,
            amount,
            0,
            0,
        )?;

        // One base-layer transaction is everything the client sends. The payer
        // is delegated on the rollup, so `helper` covers this base-layer fee.
        send(base, &[transfer.ix], &fx.helper.pubkey(), &[&fx.helper, &fx.payer])
            .context("private queued transfer (ix 25)")?;

        // The deposit is escrowed on the base immediately. `exact_out` means the
        // sender covers the transfer fee on top, so the vault holds a little
        // more than `amount` — the recipient is the one who gets exactly it.
        let escrowed = token_balance(base, &fx.vault_ata)? - vault_before;
        assert!(
            escrowed >= amount,
            "deposit escrows into the vault (got {escrowed}, need at least {amount})"
        );
        assert_eq!(
            token_balance(base, &fx.sender_ata)?,
            sender_before - escrowed,
            "sender was debited exactly what the vault received"
        );

        // The rollup decrypts the destination, enqueues, cranks it, and settles
        // back — all without further input. The recipient's balance is the
        // authoritative end state: waiting on the queue to be *empty* would pass
        // instantly if the post-action had never enqueued anything.
        let settled = wait_for(SETTLE_TIMEOUT, "the recipient to be credited on the base", || {
            token_balance(base, &fx.recipient_ata).ok().filter(|b| *b > 0)
        })?;

        assert_eq!(
            settled, amount,
            "with exact_out, the recipient receives exactly the requested amount"
        );
        assert_eq!(
            queue_header(er, &fx.queue)?.length,
            0,
            "the crank drained the queue it settled from"
        );
        // Against the pre-transfer balance, not zero: the vault also backs the
        // sender's rollup-side eATA deposit, which this transfer never touches.
        assert_eq!(
            token_balance(base, &fx.vault_ata)?,
            vault_before + escrowed - settled,
            "the vault paid the settlement out, keeping only the fee"
        );
        Ok(())
    });
}

/// A direct `DepositAndQueueTransfer` from a rollup balance (the 12-account
/// shape the SDK builds) sponsors nothing, so a recipient without a token
/// account cannot be paid. The delivery fails on the base and its callback has
/// to refund the sender. That callback must not list the receipt permission a
/// direct call never creates: the rollup rejects a transaction that marks a
/// nonexistent account writable, and the refund would never be scheduled.
#[test]
#[ignore = "spawns live validators; run with `make test-e2e`"]
fn direct_transfer_to_a_wallet_without_a_token_account_is_refunded() {
    with_stack(|base, er| {
        let fx = setup_queue(base, er, DECIMALS)?;
        let amount = 10 * 10u64.pow(DECIMALS as u32);
        let sender_before = token_balance(base, &fx.sender_ata)?;
        assert!(account_data(base, &fx.recipient_ata).is_none());

        // As on mainnet, the global vault's eATA is delegated, so the rollup
        // projects it over the vault's token account, which the delivery
        // callback lists as writable.
        send(
            base,
            &[DelegateEphemeralAtaBuilder {
                payer: fx.helper.pubkey(),
                user: fx.vault,
                mint: fx.mint,
                validator: Some(fx.validator),
            }
            .instruction()],
            &fx.helper.pubkey(),
            &[&fx.helper],
        )
        .context("delegate the global vault's eATA")?;

        send(
            er,
            &[EnsureTransferQueueCrankBuilder {
                payer: fx.payer.pubkey(),
                queue: fx.queue,
                magic_fee_vault: magic_fee_vault_pda_from_validator(&fx.validator),
                magic_context: MAGIC_CONTEXT_ID,
                magic_program: MAGIC_PROGRAM_ID,
            }
            .instruction()],
            &fx.payer.pubkey(),
            &[&fx.payer],
        )
        .context("EnsureTransferQueueCrank")?;
        fund_queue_crank(er, &fx.payer, &fx.queue)?;

        // No receipt permission and no sponsor. It spends the sender's rollup
        // balance, held by their delegated eATA's projection onto their token
        // account.
        let group_id = 1;
        let transfer = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(fx.queue, false),
                AccountMeta::new_readonly(fx.queue, false),
                AccountMeta::new_readonly(fx.mint, false),
                AccountMeta::new(fx.sender_ata, false),
                AccountMeta::new(find_vault_ata(&fx.mint, &fx.queue), false),
                AccountMeta::new_readonly(fx.recipient, false),
                AccountMeta::new_readonly(fx.payer.pubkey(), true),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new(fx.sender_ata, false),
                AccountMeta::new(derive_group_receipt(&fx.queue, &fx.payer.pubkey(), group_id), false),
                AccountMeta::new(EPHEMERAL_VAULT_ID, false),
                AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
            ],
            data: ESplInstruction::DepositAndQueueTransfer.with_data(
                &DepositAndQueueTransferArgs {
                    amount,
                    group_id: group_id.to_le_bytes()[..3].try_into()?,
                    min_delay_ms: 0,
                    max_delay_ms: 0,
                    split: 1,
                    flags: None,
                    client_ref_id: None,
                }
                .encode()?,
            ),
        };
        send(er, &[transfer], &fx.payer.pubkey(), &[&fx.payer]).context("direct DepositAndQueueTransfer")?;

        // Refunds land in the sender's token account on the base.
        let refunded = wait_for(SETTLE_TIMEOUT * 2, "the sender to be refunded on the base", || {
            token_balance(base, &fx.sender_ata)
                .ok()
                .filter(|balance| *balance > sender_before)
        })?;

        assert_eq!(
            refunded,
            sender_before + amount,
            "the sender gets the whole amount back"
        );
        assert!(
            account_data(base, &fx.recipient_ata).is_none(),
            "nothing sponsored the recipient's token account"
        );
        Ok(())
    });
}
