use ephemeral_spl_api::{instruction, state::shuttle_ephemeral_ata::ShuttleMetadata, ID as PROGRAM};
use serial_test::serial;
use solana_account::AccountSharedData;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::{tokio, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::{InstructionError, Transaction, TransactionError};

mod common;
mod utils;

use common::magic_mock::{self, take_captured_intent_bundles};

const DECIMALS: u8 = 6;
const SHUTTLE_ID: u32 = u32::MAX;

struct Fixture {
    context: ProgramTestContext,
    payer_kp: solana_keypair::Keypair,
    shuttle_metadata: Pubkey,
    shuttle_eata: Pubkey,
    shuttle_wallet_ata: Pubkey,
    refund_token: Pubkey,
}

/// An initialized shuttle owned and paid for by the payer, with the payer's token
/// account as the refund destination.
async fn setup(label: &str) -> Fixture {
    let mut context = utils::start_program_test_with(PROGRAM, magic_mock::add_mock).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair(&format!("{label}::mint"));
    let mint = mint_kp.pubkey();

    let tokens = utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, 0, 1).await;
    let (shuttle_metadata, _) = ShuttleMetadata::find_pda(&payer, &mint, SHUTTLE_ID);
    let (shuttle_eata, _) = utils::derive_shuttle_eata(PROGRAM, shuttle_metadata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);

    let mut data = instruction::ESplInstruction::InitializeShuttleEphemeralAta.to_vec();
    data.extend_from_slice(&SHUTTLE_ID.to_le_bytes());
    let init = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle_metadata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data,
    };
    let tx = Transaction::new_signed_with_payer(&[init], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    Fixture {
        context,
        payer_kp,
        shuttle_metadata,
        shuttle_eata,
        shuttle_wallet_ata,
        refund_token: tokens.user_tokens[0],
    }
}

fn undelegate_and_close_ix(fixture: &Fixture, shuttle_eata: Pubkey) -> Instruction {
    let payer = fixture.payer_kp.pubkey();
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(fixture.shuttle_metadata, false),
            AccountMeta::new_readonly(shuttle_eata, false),
            AccountMeta::new(fixture.shuttle_wallet_ata, false),
            AccountMeta::new_readonly(fixture.refund_token, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(utils::magic_context_id(), false),
            AccountMeta::new_readonly(utils::magic_program_id(), false),
        ],
        data: instruction::ESplInstruction::UndelegateAndCloseShuttleToOwner.to_vec(),
    }
}

async fn send(fixture: &mut Fixture, ix: Instruction) -> Result<(), TransactionError> {
    let payer = fixture.payer_kp.pubkey();
    let blockhash = fixture.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&fixture.payer_kp], blockhash);
    fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .map_err(|e| e.unwrap())
}

/// A reused shuttle address can reach the ER while it still holds the previous
/// round's closed shuttle EATA. The undelegation must not depend on that copy.
#[tokio::test]
#[serial]
async fn undelegate_and_close_shuttle_ignores_a_stale_closed_shuttle_eata() {
    let mut fixture = setup("undelegate_and_close_shuttle_stale_eata").await;
    fixture
        .context
        .set_account(&fixture.shuttle_eata, &AccountSharedData::default());
    take_captured_intent_bundles(utils::magic_program_id());

    let ix = undelegate_and_close_ix(&fixture, fixture.shuttle_eata);
    send(&mut fixture, ix).await.unwrap();

    let bundles = take_captured_intent_bundles(utils::magic_program_id());
    assert_eq!(bundles.len(), 1, "expected exactly one intent bundle");
    let undelegate = bundles[0]
        .args
        .commit_and_undelegate
        .as_ref()
        .expect("the shuttle wallet ATA must be committed and undelegated");
    let committed: Vec<Pubkey> = undelegate
        .committed_accounts_indices()
        .iter()
        .map(|&i| bundles[0].schedule_accounts[i as usize])
        .collect();
    assert_eq!(committed, vec![fixture.shuttle_wallet_ata]);
}

/// Without reading the shuttle EATA, its address stays bound to the shuttle and the
/// mint of the shuttle wallet ATA.
#[tokio::test]
#[serial]
async fn undelegate_and_close_shuttle_rejects_a_foreign_shuttle_eata() {
    let mut fixture = setup("undelegate_and_close_shuttle_foreign_eata").await;
    take_captured_intent_bundles(utils::magic_program_id());

    let foreign = utils::test_pubkey("undelegate_and_close_shuttle_foreign_eata::eata");
    let ix = undelegate_and_close_ix(&fixture, foreign);
    assert_eq!(
        send(&mut fixture, ix).await.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidSeeds)
    );
    assert!(take_captured_intent_bundles(utils::magic_program_id()).is_empty());
}
