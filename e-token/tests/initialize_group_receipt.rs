use crate::common::crank_mock::take_execute_cranks;
use crate::common::magic_mock::take_captured_ephemeral_closes;
use crate::common::{crank_mock, magic_mock};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, GroupReceiptHeader, TransferReceipt};
use ephemeral_spl_api::state::transfer_queue::{HEADER_LEN, QUEUE_SEED, TRANSFER_QUEUE_VERSION};
use ephemeral_spl_api::ID as PROGRAM;
use magicblock_magic_program_api::instruction::MagicBlockInstruction;
use magicblock_magic_program_api::pda::CRANK_SIGNER;
use magicblock_magic_program_api::CRANK_PROGRAM_ID;
use serial_test::serial;
use solana_account::Account as SolanaAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::rent::Rent;
use solana_program_test::{tokio, ProgramTest};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_transaction::Transaction;
use utils::TestInternalInstruction as internal;

mod common;
mod utils;

const MAGIC_VAULT: Pubkey = pubkey!("MagicVau1t999999999999999999999999999999999");

const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

// ── helpers ───────────────────────────────────────────────────────────────────

/// Serialise the initialize_group_receipt instruction data.
/// Layout: discriminator(1) + group_id(4) + splits(4)
fn initialize_group_receipt_ix_data(group_id: u32, splits: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(9);
    data.push(internal::InitializeGroupReceipt.discriminator());
    data.extend_from_slice(&group_id.to_le_bytes());
    data.extend_from_slice(&splits.to_le_bytes());
    data
}

fn derive_queue(mint: Pubkey, validator: Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref(), validator.as_ref()], &PROGRAM)
}

fn derive_group_receipt(queue: Pubkey, group_id: u32) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            GROUP_RECEIPT_SEED,
            queue.as_ref(),
            group_id.to_le_bytes().as_ref(),
        ],
        &PROGRAM,
    )
}

/// Build a minimal queue account buffer with the fields validated by the program.
fn queue_account_data(mint: Pubkey, validator: Pubkey, bump: u8) -> Vec<u8> {
    let mut data = vec![0u8; HEADER_LEN];
    // TransferQueueHeader layout (repr C):
    // version(1) bump(1) _pad0(6) mint(32) length(4) _pad1(8) next_task_id(4) crank_task_id(8) validator(32)
    data[0] = TRANSFER_QUEUE_VERSION;
    data[1] = bump;
    data[8..40].copy_from_slice(&mint.to_bytes());
    data[64..96].copy_from_slice(&validator.to_bytes());
    data
}

/// Build a receipt buffer with `splits=0` (partially initialized) and enough
/// space for `capacity` items. Used to test the "already exists, set splits"
/// path of process_initialize_group_receipt.
fn receipt_account_data_partial(group_id: u32, bump: u8, capacity: u32) -> Vec<u8> {
    // splits=0 means not yet fully initialized
    let mut data = vec![0u8; GroupReceipt::required_size(capacity as usize)];
    let header = GroupReceiptHeader::new(group_id, bump, 0);
    data[..GroupReceiptHeader::size()].copy_from_slice(bytemuck::bytes_of(&header));
    data
}

async fn setup_context(
    receipt_data: Vec<u8>,
    group_id: u32,
) -> (
    solana_program_test::ProgramTestContext,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let (receipt, _) = derive_group_receipt(queue, group_id);

    let rent = Rent::default();

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", PROGRAM, None);
    magic_mock::add_mock(&mut pt);
    crank_mock::add_mock(&mut pt);

    let queue_data = queue_account_data(mint, validator.pubkey(), queue_bump);
    pt.add_account(
        queue,
        SolanaAccount {
            lamports: rent.minimum_balance(queue_data.len()),
            data: queue_data,
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        receipt,
        SolanaAccount {
            lamports: rent.minimum_balance(receipt_data.len()),
            data: receipt_data,
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        MAGIC_VAULT,
        SolanaAccount {
            lamports: rent.minimum_balance(0).max(1),
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        validator.pubkey(),
        SolanaAccount {
            lamports: 1_000_000,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    let ctx = pt.start_with_context().await;

    magic_mock::clear_all_captured(MAGIC_PROGRAM_ID);

    (ctx, validator, queue, receipt, mint)
}

fn initialize_group_receipt_ix(
    queue: Pubkey,
    receipt: Pubkey,
    group_id: u32,
    splits: u32,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(CRANK_SIGNER, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(receipt, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ],
        data: initialize_group_receipt_ix_data(group_id, splits),
    }
}

fn crank_executor_ix(
    validator: Pubkey,
    queue: Pubkey,
    receipt: Pubkey,
    group_id: u32,
    splits: u32,
) -> Instruction {
    let inner_ix = initialize_group_receipt_ix(queue, receipt, group_id, splits);

    let mut account_metas = vec![
        AccountMeta::new_readonly(validator, true),
        AccountMeta::new_readonly(CRANK_SIGNER, false),
        AccountMeta::new_readonly(inner_ix.program_id, false),
    ];
    account_metas.extend(inner_ix.accounts.clone().into_iter().map(|mut el| {
        // CRANK_SIGNER may be set to true in inner_instruction
        // Outer instruction can't have PDA as signer
        el.is_signer = false;
        el
    }));

    Instruction::new_with_bincode(
        CRANK_PROGRAM_ID,
        &MagicBlockInstruction::ExecuteCrank {
            instructions: vec![inner_ix],
        },
        account_metas,
    )
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// process_initialize_group_receipt: receipt account is pre-created in the
/// test context (simulating what the magic program would have allocated).
/// The processor takes the "already owned by program" path, sets splits,
/// and does not close because no transfers have been recorded yet.
#[tokio::test]
#[serial]
async fn initialize_group_receipt_sets_splits_on_existing_receipt() {
    let group_id: u32 = 42;
    let splits: u32 = 3;

    // Pre-create with splits=0 (partially initialised) and enough capacity for `splits` items.
    let receipt_data = receipt_account_data_partial(group_id, 0, splits);
    let (ctx, validator, queue, receipt, _mint) = setup_context(receipt_data, group_id).await;

    let ix = crank_executor_ix(validator.pubkey(), queue, receipt, group_id, splits);

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let executed_cranks = take_execute_cranks();
    assert_eq!(executed_cranks.len(), 1);

    // No close — no transfers have been recorded.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert!(closes.is_empty(), "expected no CloseEphemeralAccount CPI");

    // Receipt must now have splits set correctly.
    let account = ctx
        .banks_client
        .get_account(receipt)
        .await
        .unwrap()
        .expect("receipt must still exist");
    let header =
        bytemuck::try_from_bytes::<GroupReceiptHeader>(&account.data[..GroupReceiptHeader::size()])
            .unwrap();
    assert_eq!(header.splits(), splits);
    assert_eq!(header.transfer_completed(), 0);
}

/// process_initialize_group_receipt: receipt pre-created with splits=0 and
/// one transfer already recorded. When initialized with splits=1, the program
/// detects all callbacks are done and CPIs CloseEphemeralAccount.
#[tokio::test]
#[serial]
async fn initialize_group_receipt_closes_when_all_callbacks_already_done() {
    let group_id: u32 = 7;
    let splits: u32 = 1;

    // Build a receipt that has 1 transfer recorded but splits still 0.
    let mut receipt_data = receipt_account_data_partial(group_id, 0, splits);
    // Manually write transfers_completed = 1 into the header.
    // GroupReceiptHeader layout: id(4) + splits(4) + transfers_completed(4) + bump(1) + _reserved(11)
    // transfers_completed is at offset 8.
    receipt_data[8..12].copy_from_slice(&1u32.to_le_bytes());
    // Write one dummy TransferReceipt item after the header.
    let item = TransferReceipt::new(None, 99, true);
    let header_size = GroupReceiptHeader::size();
    receipt_data[header_size..header_size + TransferReceipt::size()]
        .copy_from_slice(bytemuck::bytes_of(&item));

    let (ctx, validator, queue, receipt, _mint) = setup_context(receipt_data, group_id).await;

    let ix = crank_executor_ix(validator.pubkey(), queue, receipt, group_id, splits);

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );

    let res = ctx
        .banks_client
        .process_transaction_with_metadata(tx)
        .await
        .unwrap();
    res.result.unwrap();

    let executed_cranks = take_execute_cranks();
    assert_eq!(executed_cranks.len(), 1);

    // CloseEphemeralAccount must be called — all callbacks already done.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(closes.len(), 1, "expected one CloseEphemeralAccount CPI");

    // Log confirming all transfers complete.
    let logs = res
        .metadata
        .as_ref()
        .map(|m| &m.log_messages)
        .expect("expected log messages");
    assert!(
        logs.iter()
            .any(|l| l.contains("All transfers complete for group id")),
        "expected 'All transfers complete' log; got:\n{}",
        logs.join("\n"),
    );
}
