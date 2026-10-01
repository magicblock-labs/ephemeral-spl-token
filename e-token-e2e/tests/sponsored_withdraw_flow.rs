//! A sponsored withdrawal (ix 37) for a wallet whose only funds are in the
//! rollup: the sponsor pays every lamport and recreates the owner's base ATA,
//! and the fee moves from the rollup balance into the transfer queue's vault.
//!
//! Instruction 26's `CloseMagicAta` post-action needs a rollup newer than any
//! published `ephemeral-validator` (<= 0.14.10), so `make test-e2e` skips this
//! file. Run it against a `magicblock-validator` build on the rollup port
//! (start `make e2e-er-validator` once first to create the validator fee
//! vault): `E2E_SKIP_ER_VALIDATOR=1 cargo test -p ephemeral-token-e2e --test
//! sponsored_withdraw_flow -- --ignored`.

use std::time::Duration;

use anyhow::{Context, Result};
use ephemeral_rollups_sdk::{
    consts::{ASSOCIATED_TOKEN_PROGRAM_ID, TOKEN_PROGRAM_ID},
    spl::{
        builders::{
            DelegateEphemeralAtaBuilder, DepositSplTokensBuilder, InitializeEphemeralAtaBuilder,
            WithdrawThroughDelegatedShuttleWithMergeBuilder,
        },
        find_vault_ata,
    },
};
use ephemeral_spl_api::instruction::ESplInstruction;
use ephemeral_token_e2e::{
    base_programs,
    fixture::{setup_queue, token_balance, SYSTEM_PROGRAM_ID},
    rpc::{account_data, send, wait_for},
    stack::{Stack, StackConfig},
    STACK_LOCK,
};
use solana_client::rpc_client::RpcClient;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use spl_token_interface::instruction::{close_account, transfer};

const DECIMALS: u8 = 6;
const DEPOSIT: u64 = 10_000_000;
const WITHDRAW_AMOUNT: u64 = 2_000_000;
const FEE: u64 = 200_000;
const SETTLE_TIMEOUT: Duration = Duration::from_secs(90);

fn with_stack<T>(body: impl FnOnce(&RpcClient, &RpcClient) -> Result<T>) -> T {
    let _guard = STACK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let programs = base_programs().expect("build artifacts present");
    let stack = Stack::start(StackConfig::from_env(), &programs).expect("stack starts");
    let result = body(&stack.base_rpc(), &stack.er_rpc());
    drop(stack);
    result.expect("e2e sponsored withdraw flow")
}

fn create_ata_idempotent_ix(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey) -> Instruction {
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(find_vault_ata(mint, owner), false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data: vec![1], // CreateIdempotent
    }
}

#[test]
#[ignore = "spawns live validators; see the module docs"]
fn sponsored_withdraw_pays_the_fee_from_the_rollup_balance() {
    with_stack(|base, er| {
        let fx = setup_queue(base, er, DECIMALS)?;
        let sponsor = &fx.helper;
        let owner = Keypair::new();
        let owner_ata = find_vault_ata(&fx.mint, &owner.pubkey());
        let queue_vault_ata = find_vault_ata(&fx.mint, &fx.queue);

        // Move tokens into the owner's rollup balance and close the base ATA:
        // the owner ends up with nothing on the base, not even a lamport.
        let fund = [
            create_ata_idempotent_ix(&sponsor.pubkey(), &owner.pubkey(), &fx.mint),
            transfer(
                &TOKEN_PROGRAM_ID,
                &fx.sender_ata,
                &owner_ata,
                &fx.payer.pubkey(),
                &[],
                DEPOSIT,
            )?,
            InitializeEphemeralAtaBuilder {
                payer: sponsor.pubkey(),
                user: owner.pubkey(),
                mint: fx.mint,
            }
            .instruction(),
            DepositSplTokensBuilder {
                authority: owner.pubkey(),
                user: owner.pubkey(),
                mint: fx.mint,
                amount: DEPOSIT,
            }
            .instruction(),
            DelegateEphemeralAtaBuilder {
                payer: sponsor.pubkey(),
                user: owner.pubkey(),
                mint: fx.mint,
                validator: Some(fx.validator),
            }
            .instruction(),
            close_account(&TOKEN_PROGRAM_ID, &owner_ata, &sponsor.pubkey(), &owner.pubkey(), &[])?,
        ];
        send(base, &fund, &sponsor.pubkey(), &[sponsor, &fx.payer, &owner]).context("fund the owner")?;
        assert!(account_data(base, &owner_ata).is_none(), "the owner holds no base ATA");
        let vault_before = token_balance(er, &queue_vault_ata)?;

        for (round, shuttle_id) in [(1, 1_u32), (2, 2)] {
            // The published SDK has no instruction 37 builder yet: extend its instruction 26.
            let mut withdraw = WithdrawThroughDelegatedShuttleWithMergeBuilder {
                payer: sponsor.pubkey(),
                owner: owner.pubkey(),
                owner_ata,
                mint: fx.mint,
                shuttle_id,
                amount: WITHDRAW_AMOUNT,
                validator: Some(fx.validator),
            }
            .instruction();
            withdraw.accounts.push(AccountMeta::new_readonly(fx.queue, false));
            withdraw.data[0] = ESplInstruction::WithdrawThroughDelegatedShuttleWithFee.value();
            withdraw.data.extend_from_slice(&FEE.to_le_bytes());
            send(
                base,
                &[
                    create_ata_idempotent_ix(&sponsor.pubkey(), &owner.pubkey(), &fx.mint),
                    withdraw,
                ],
                &sponsor.pubkey(),
                &[sponsor, &owner],
            )
            .with_context(|| format!("sponsored withdraw #{round} (ix 37)"))?;

            wait_for(SETTLE_TIMEOUT, "the owner to be credited on the base", || {
                token_balance(base, &owner_ata)
                    .ok()
                    .filter(|b| *b == round * (WITHDRAW_AMOUNT - FEE))
            })?;
            assert_eq!(token_balance(er, &queue_vault_ata)?, vault_before + round * FEE);
            assert_eq!(token_balance(er, &owner_ata)?, DEPOSIT - round * WITHDRAW_AMOUNT);
        }

        assert_eq!(
            base.get_balance(&owner.pubkey()).unwrap_or(0),
            0,
            "the owner never paid a lamport"
        );
        Ok(())
    });
}
