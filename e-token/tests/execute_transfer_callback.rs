use crate::common::callback_mock::take_execute_callbacks;
use crate::common::magic_mock::{take_captured_ephemeral_closes, take_captured_ephemeral_creates};
use crate::common::{callback_mock, magic_mock};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, GroupReceiptHeader};
use ephemeral_spl_api::state::transfer_queue::{HEADER_LEN, QUEUE_SEED, TRANSFER_QUEUE_VERSION};
use ephemeral_spl_api::ID as PROGRAM;
use ephemeral_token_program::TransferCallbackArgs;
use magicblock_magic_program_api::instruction::CallbackInstruction;
use magicblock_magic_program_api::pda::CALLBACK_SIGNER;
use magicblock_magic_program_api::CALLBACK_PROGRAM_ID;
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

/// Serialise the callback instruction data for `callback_ix_data`: discriminator
/// (`internal::EXECUTE_TRANSFER_CALLBACK`) + `MagicResponseView`.
///
/// `MagicResponseView.data` contains the serialised `TransferCallbackArgs` payload:
/// amount(8) + group_id(4) + flag(1).
fn callback_ix_data(ok: bool, amount: u64, group_id: u32) -> Vec<u8> {
    // TransferCallbackArgs payload
    let mut args = Vec::with_capacity(13);
    args.extend_from_slice(&amount.to_le_bytes());
    args.extend_from_slice(&group_id.to_le_bytes());
    args.push(0u8); // flag

    let args2 = TransferCallbackArgs {
        amount,
        group_id,
        flag: 0,
    }
    .encode()
    .unwrap();

    assert_eq!(args, args2, "Invalid serialization");

    // MagicResponseView (bincode V1): variant(4) + ok(1) + data_len(8) + data + error_len(8) + sig_tag(1)
    let mut data = Vec::new();
    data.push(internal::ExecuteTransferCallback.discriminator());
    data.extend_from_slice(&0u32.to_le_bytes()); // variant = 0
    data.push(ok as u8);
    data.extend_from_slice(&(args.len() as u64).to_le_bytes());
    data.extend_from_slice(&args);
    data.extend_from_slice(&0u64.to_le_bytes()); // error_len = 0
    data.push(0u8); // no signature
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

/// Build a pre-initialised group receipt buffer.
fn receipt_account_data(group_id: u32, splits: u32, bump: u8) -> Vec<u8> {
    let mut data = vec![0u8; GroupReceipt::required_size(splits as usize)];
    let header = GroupReceiptHeader::new(group_id, bump, splits);
    data[..GroupReceiptHeader::size()].copy_from_slice(bytemuck::bytes_of(&header));
    data
}

/// Common fixture: a ProgramTest context with queue and receipt accounts
/// pre-populated. Returns (context, validator keypair, mint, queue, receipt).
async fn setup_context(
    receipt_data: Vec<u8>,
    group_id: u32,
) -> (
    solana_program_test::ProgramTestContext,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let (receipt, _) = derive_group_receipt(queue, group_id);
    let vault = Keypair::new().pubkey();
    let vault_token = utils::derive_associated_token_address(vault, mint);

    let rent = Rent::default();

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", PROGRAM, None);
    magic_mock::add_mock(&mut pt);
    callback_mock::add_mock(&mut pt);

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

    for pk in [vault, mint, vault_token, MAGIC_VAULT] {
        pt.add_account(
            pk,
            SolanaAccount {
                lamports: rent.minimum_balance(0).max(1),
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

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

    (ctx, validator, mint, queue, receipt, vault, vault_token)
}

fn callback_executor_ix(
    validator: Pubkey,
    receipt: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    mint: Pubkey,
    vault_token: Pubkey,
    ok: bool,
    amount: u64,
    group_id: u32,
) -> Instruction {
    let callback_ix = callback_ix(
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        ok,
        amount,
        group_id,
    );

    // Mandatory accounts for magic-program
    let mut account_metas = vec![
        AccountMeta::new_readonly(validator, true),
        AccountMeta::new_readonly(CALLBACK_SIGNER, false),
        AccountMeta::new_readonly(callback_ix.program_id, false),
    ];
    account_metas.extend(callback_ix.accounts.clone().into_iter().map(|mut el| {
        // CALLBACK_SIGNER may be set to true in inner_instruction
        // Outer instruction can't have PDA as signer
        el.is_signer = false;
        el
    }));

    Instruction::new_with_bincode(
        CALLBACK_PROGRAM_ID,
        &CallbackInstruction::ExecuteCallback {
            instruction: callback_ix,
        },
        account_metas,
    )
}

fn callback_ix(
    receipt: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    mint: Pubkey,
    vault_token: Pubkey,
    ok: bool,
    amount: u64,
    group_id: u32,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(CALLBACK_SIGNER, true),
            AccountMeta::new(receipt, false),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_token, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
        ],
        data: callback_ix_data(ok, amount, group_id),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Mid-group callback: receipt already exists (pre-initialized via
/// process_initialize_group_receipt TX), not the last transfer.
/// No CreateEphemeralAccount CPI to the magic program — only the receipt state changes.
#[tokio::test]
#[serial]
async fn execute_callback_with_pre_initialized_receipt_no_magic_cpi() {
    let group_id: u32 = 1;
    let splits: u32 = 2;

    // Pre-create receipt as if magic program already created it and
    // process_initialize_group_receipt ran.
    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    // Simulate process_initialize_group_receipt having already run (receipt
    // is owned by PROGRAM with splits set). Now shoot the callback directly.
    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        true,
        100,
        group_id,
    );

    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let executed_callbacks = take_execute_callbacks();
    assert_eq!(executed_callbacks.len(), 1);

    // No CreateEphemeralAccount should have been called — receipt already existed.
    let creates = take_captured_ephemeral_creates(MAGIC_PROGRAM_ID);
    assert!(creates.is_empty(), "expected no CreateEphemeralAccount CPI");

    // No CloseEphemeralAccount either — this is not the last transfer.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert!(closes.is_empty(), "expected no CloseEphemeralAccount CPI");

    // Receipt transfer_completed should be 1.
    let account = ctx
        .banks_client
        .get_account(receipt)
        .await
        .unwrap()
        .expect("receipt must still exist");
    let header =
        bytemuck::try_from_bytes::<GroupReceiptHeader>(&account.data[..GroupReceiptHeader::size()])
            .unwrap();
    assert_eq!(header.transfer_completed(), 1);
}

/// Last-transfer callback: receipt pre-initialized with splits=1.
/// After callback the program CPIs CloseEphemeralAccount.
#[tokio::test]
#[serial]
async fn execute_callback_closes_receipt_when_last_transfer_with_pre_initialized_receipt() {
    let group_id: u32 = 1;
    let splits: u32 = 1;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        true,
        200,
        group_id,
    );

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

    let callbacks = take_execute_callbacks();
    assert_eq!(callbacks.len(), 1);

    // No CreateEphemeralAccount — receipt was pre-initialized.
    let creates = take_captured_ephemeral_creates(MAGIC_PROGRAM_ID);
    assert!(creates.is_empty(), "expected no CreateEphemeralAccount CPI");

    // CloseEphemeralAccount must have been called once — this was the last transfer.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(
        closes.len(),
        1,
        "expected exactly one CloseEphemeralAccount CPI"
    );
}

/// Mid-group callback: receipt already exists, not the last transfer.
/// No CPI to the magic program — only the receipt state changes.
#[tokio::test]
#[serial]
async fn execute_callback_records_transfer() {
    let group_id: u32 = 1;
    let splits: u32 = 2;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        true,
        100,
        group_id,
    );

    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let executed_callbacks = take_execute_callbacks();
    assert_eq!(executed_callbacks.len(), 1);

    let account = ctx
        .banks_client
        .get_account(receipt)
        .await
        .unwrap()
        .unwrap();
    let header =
        bytemuck::try_from_bytes::<GroupReceiptHeader>(&account.data[..GroupReceiptHeader::size()])
            .unwrap();
    assert_eq!(header.transfer_completed(), 1);
}

/// Last-transfer callback: all splits complete. The program logs "All transfers complete…"
/// confirming it reached the close path. The mock's CloseEphemeralAccount is a no-op (see
/// `common::magic_mock` limitations), so we verify via the program log rather than lamports.
#[tokio::test]
#[serial]
async fn execute_callback_closes_receipt_on_last_transfer() {
    let group_id: u32 = 1;
    let splits: u32 = 1;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        true,
        200,
        group_id,
    );

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

    // Transaction must succeed.
    res.result.unwrap();

    let executed_callbacks = take_execute_callbacks();
    assert_eq!(executed_callbacks.len(), 1);

    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(closes.len(), 1);
}
