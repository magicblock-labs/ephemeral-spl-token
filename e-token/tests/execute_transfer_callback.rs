use ephemeral_spl_api::instruction::internal;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, GroupReceiptHeader};
use ephemeral_spl_api::state::transfer_queue::{header_len, QUEUE_SEED, TRANSFER_QUEUE_VERSION};
use solana_account::Account as SolanaAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::rent::Rent;
use solana_program_test::{processor, tokio, ProgramTest};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::sync::{Mutex, OnceLock};

mod common;
mod utils;

const PROGRAM: Pubkey = pubkey!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");
const MAGIC_PROGRAM: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");
const MAGIC_VAULT: Pubkey = pubkey!("MagicVau1t999999999999999999999999999999999");

const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Serialise the callback instruction data: discriminator + MagicResponseView + TransferCallbackArgs.
///
/// TransferCallbackArgs layout (13 bytes): amount(8) + group_id(4) + flag(1)
fn callback_ix_data(ok: bool, amount: u64, group_id: u32) -> Vec<u8> {
    // TransferCallbackArgs payload
    let mut args = Vec::with_capacity(13);
    args.extend_from_slice(&amount.to_le_bytes());
    args.extend_from_slice(&group_id.to_le_bytes());
    args.push(0u8); // flag

    // MagicResponseView (bincode V1): variant(4) + ok(1) + data_len(8) + data + error_len(8) + sig_tag(1)
    let mut data = Vec::new();
    data.push(internal::EXECUTE_TRANSFER_CALLBACK);
    data.extend_from_slice(&0u32.to_le_bytes()); // variant = 0
    data.push(ok as u8);
    data.extend_from_slice(&(args.len() as u64).to_le_bytes());
    data.extend_from_slice(&args);
    data.extend_from_slice(&0u64.to_le_bytes()); // error_len = 0
    data.push(0u8); // no signature
    data
}

/// Serialise the initialize_group_receipt instruction data.
/// Layout: discriminator(1) + group_id(4) + splits(4)
fn initialize_group_receipt_ix_data(group_id: u32, splits: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(9);
    data.push(internal::INITIALIZE_GROUP_RECEIPT);
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
    let mut data = vec![0u8; header_len()];
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

fn add_mock_program(pt: &mut ProgramTest) {
    pt.prefer_bpf(false);
    pt.add_program(
        "magic_mock",
        MAGIC_PROGRAM,
        processor!(common::magic_mock::process),
    );
    pt.prefer_bpf(true);
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
    add_mock_program(&mut pt);

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

    common::magic_mock::clear_all_captured(MAGIC_PROGRAM);

    (ctx, validator, mint, queue, receipt, vault, vault_token)
}

fn callback_ix(
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
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(validator, true),
            AccountMeta::new(receipt, false),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_token, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: callback_ix_data(ok, amount, group_id),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Mid-group callback: receipt already exists (pre-initialized via
/// process_initialize_group_receipt TX), not the last transfer.
/// No CreateEphemeralAccount CPI to the magic program — only the receipt state changes.
#[tokio::test]
async fn execute_callback_with_pre_initialized_receipt_no_magic_cpi() {
    let _test_guard = test_lock().lock().unwrap();
    let group_id: u32 = 1;
    let splits: u32 = 2;

    // Pre-create receipt as if magic program already created it and
    // process_initialize_group_receipt ran.
    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    // Simulate process_initialize_group_receipt having already run (receipt
    // is owned by PROGRAM with splits set). Now shoot the callback directly.
    let ix = callback_ix(
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
        &[ix],
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // No CreateEphemeralAccount should have been called — receipt already existed.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert!(creates.is_empty(), "expected no CreateEphemeralAccount CPI");

    // No CloseEphemeralAccount either — this is not the last transfer.
    let closes = common::magic_mock::take_captured_ephemeral_closes(MAGIC_PROGRAM);
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
async fn execute_callback_closes_receipt_when_last_transfer_with_pre_initialized_receipt() {
    let _test_guard = test_lock().lock().unwrap();
    let group_id: u32 = 1;
    let splits: u32 = 1;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = callback_ix(
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

    // No CreateEphemeralAccount — receipt was pre-initialized.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert!(creates.is_empty(), "expected no CreateEphemeralAccount CPI");

    // CloseEphemeralAccount must have been called once — this was the last transfer.
    let closes = common::magic_mock::take_captured_ephemeral_closes(MAGIC_PROGRAM);
    assert_eq!(
        closes.len(),
        1,
        "expected exactly one CloseEphemeralAccount CPI"
    );

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

/// process_initialize_group_receipt: receipt account is pre-created in the
/// test context (simulating what the magic program would have allocated).
/// The processor takes the "already owned by program" path, sets splits,
/// and does not close because no transfers have been recorded yet.
#[tokio::test]
async fn initialize_group_receipt_sets_splits_on_existing_receipt() {
    let _test_guard = test_lock().lock().unwrap();
    let group_id: u32 = 42;
    let splits: u32 = 3;

    // Pre-create with splits=0 (partially initialised) and enough capacity for `splits` items.
    let receipt_data = receipt_account_data_partial(group_id, 0, splits);
    let (ctx, validator, _mint, queue, receipt, _vault, _vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(validator.pubkey(), true),
            AccountMeta::new(queue, false),
            AccountMeta::new(receipt, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: initialize_group_receipt_ix_data(group_id, splits),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // No close — no transfers have been recorded.
    let closes = common::magic_mock::take_captured_ephemeral_closes(MAGIC_PROGRAM);
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
async fn initialize_group_receipt_closes_when_all_callbacks_already_done() {
    let _test_guard = test_lock().lock().unwrap();
    use ephemeral_spl_api::state::group_receipt::TransferReceipt;

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

    let (ctx, validator, _mint, queue, receipt, _vault, _vault_token) =
        setup_context(receipt_data, group_id).await;

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(validator.pubkey(), true),
            AccountMeta::new(queue, false),
            AccountMeta::new(receipt, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: initialize_group_receipt_ix_data(group_id, splits),
    };

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

    // CloseEphemeralAccount must be called — all callbacks already done.
    let closes = common::magic_mock::take_captured_ephemeral_closes(MAGIC_PROGRAM);
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

// ── original tests (kept) ─────────────────────────────────────────────────────

/// Mid-group callback: receipt already exists, not the last transfer.
/// No CPI to the magic program — only the receipt state changes.
#[tokio::test]
async fn execute_callback_records_transfer() {
    let _test_guard = test_lock().lock().unwrap();
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let vault = Keypair::new().pubkey();
    let vault_token = utils::derive_associated_token_address(vault, mint);
    let group_id: u32 = 1;
    let splits: u32 = 2;
    let (receipt, _) = derive_group_receipt(queue, group_id);

    let rent = Rent::default();

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", PROGRAM, None);
    add_mock_program(&mut pt);

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

    let receipt_data = receipt_account_data(group_id, splits, 0);
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

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(validator.pubkey(), true),
            AccountMeta::new(receipt, false),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_token, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: callback_ix_data(true, 100, group_id),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

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
async fn execute_callback_closes_receipt_on_last_transfer() {
    let _test_guard = test_lock().lock().unwrap();
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let vault = Keypair::new().pubkey();
    let vault_token = utils::derive_associated_token_address(vault, mint);
    let group_id: u32 = 1;
    let splits: u32 = 1;
    let (receipt, _) = derive_group_receipt(queue, group_id);

    let rent = Rent::default();

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", PROGRAM, None);
    add_mock_program(&mut pt);

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

    let receipt_data = receipt_account_data(group_id, splits, 0);
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

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(validator.pubkey(), true),
            AccountMeta::new(receipt, false),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault_token, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(MAGIC_VAULT, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: callback_ix_data(true, 200, group_id),
    };

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

    // The program emits "All transfers complete for group id: ..." when it reaches the close
    // path. Verify this log is present — it confirms the right code path was taken.
    let logs = res
        .metadata
        .as_ref()
        .map(|m| &m.log_messages)
        .expect("expected log messages in transaction metadata");
    assert!(
        logs.iter()
            .any(|l| l.contains("All transfers complete for group id")),
        "expected 'All transfers complete' log; got:\n{}",
        logs.join("\n"),
    );
}
