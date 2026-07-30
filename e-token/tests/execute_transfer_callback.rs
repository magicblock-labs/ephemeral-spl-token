use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::{
    state::{
        group_receipt::{GroupReceipt, GroupReceiptHeader},
        transfer_queue::{HEADER_LEN, QUEUE_SEED, TRANSFER_QUEUE_VERSION},
    },
    ID as PROGRAM,
};
use magicblock_magic_program_api::{instruction::CallbackInstruction, pda::CALLBACK_SIGNER, CALLBACK_PROGRAM_ID};
use serial_test::serial;
use solana_account::Account as SolanaAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::rent::Rent;
use solana_program_test::{tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use utils::TestInternalInstruction as internal;
use wheels::{layout::Encodable as _, variable_offset_layout};

use crate::common::{
    acl_mock, callback_mock,
    callback_mock::take_execute_callbacks,
    magic_mock,
    magic_mock::{take_captured_ephemeral_closes, take_captured_ephemeral_creates},
};

mod common;
mod utils;

// ── helpers ───────────────────────────────────────────────────────────────────
// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
pub(crate) struct TransferCallbackArgs {
    /// Amount was transferred in action
    pub amount: u64,
    /// Group ID of a transfer
    pub group_id: u32,
    // Flags
    pub flag: u8,
}

/// Serialise the callback instruction data for `callback_ix_data`: discriminator
/// (`internal::EXECUTE_TRANSFER_CALLBACK`) + `MagicResponseView`.
///
/// `MagicResponseView.data` contains the serialised `TransferCallbackArgs` payload:
/// amount(8) + group_id(4) + flag(1).
fn callback_ix_data(ok: bool, amount: u64, group_id: u32) -> Vec<u8> {
    // TransferCallbackArgs payload
    let args = TransferCallbackArgs {
        amount,
        group_id,
        flag: 0,
    }
    .encode()
    .unwrap();

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

/// Build a minimal queue account buffer with the fields validated by the program.
fn queue_account_data(mint: Pubkey, validator: Pubkey, bump: u8) -> Vec<u8> {
    let mut data = vec![0u8; HEADER_LEN];
    // TransferQueueHeader layout (repr C):
    // version(1) bump(1) spl_token_program(1) _pad0(5) mint(32) length(4) _pad1(8) next_task_id(4) crank_task_id(8) validator(32)
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
    data[..GroupReceiptHeader::SIZE].copy_from_slice(bytemuck::bytes_of(&header));
    data
}

/// Common fixture: a ProgramTest context with queue and receipt accounts
/// pre-populated. Returns (context, validator keypair, mint, queue, receipt, vault, vault_token, source).
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
    Pubkey,
) {
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let source = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let (receipt, _) = utils::derive_group_receipt(queue, source, group_id);
    let vault = Keypair::new().pubkey();
    let vault_token = utils::derive_associated_token_address(vault, mint);

    let rent = Rent::default();

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", PROGRAM, None);
    magic_mock::add_mock(&mut pt);
    callback_mock::add_mock(&mut pt);
    acl_mock::add_mock(&mut pt);

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

    for pk in [vault, mint, vault_token, utils::MAGIC_VAULT, source] {
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
    let _ = take_execute_callbacks();

    (ctx, validator, mint, queue, receipt, vault, vault_token, source)
}

fn magic_fee_vault_pubkey(validator: Pubkey) -> Pubkey {
    Pubkey::new_from_array(magic_fee_vault_pda_from_validator(&validator.to_bytes().into()).to_bytes())
}

fn callback_executor_ix(
    validator: Pubkey,
    receipt: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    mint: Pubkey,
    vault_token: Pubkey,
    source: Pubkey,
    ok: bool,
    amount: u64,
    group_id: u32,
) -> Instruction {
    let magic_fee_vault = magic_fee_vault_pubkey(validator);
    let magic_context = MAGIC_CONTEXT_ID;
    let callback_ix = callback_ix(
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        source,
        magic_fee_vault,
        magic_context,
        ok,
        amount,
        group_id,
    );

    callback_executor_ix_from(validator, callback_ix)
}

/// Wraps an inner callback instruction in the callback-executor instruction.
fn callback_executor_ix_from(validator: Pubkey, callback_ix: Instruction) -> Instruction {
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

/// Extends a 13-account callback instruction to the 15-account shape that also
/// closes the group receipt's ACL permission.
fn append_permission_accounts(mut ix: Instruction, permission: Pubkey) -> Instruction {
    ix.accounts.push(AccountMeta::new(permission, false)); // 13: group_receipt_permission
    ix.accounts
        .push(AccountMeta::new_readonly(utils::permission_program_id(), false)); // 14: permission_program
    ix
}

fn callback_ix(
    receipt: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    mint: Pubkey,
    vault_token: Pubkey,
    source: Pubkey,
    magic_fee_vault: Pubkey,
    magic_context: Pubkey,
    ok: bool,
    amount: u64,
    group_id: u32,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(CALLBACK_SIGNER, true),       // 0: callback_signer
            AccountMeta::new(receipt, false),              // 1: group_receipt
            AccountMeta::new(queue, false),                // 2: queue_info
            AccountMeta::new_readonly(vault, false),       // 3: vault
            AccountMeta::new_readonly(mint, false),        // 4: mint
            AccountMeta::new_readonly(vault_token, false), // 5: vault_token_account
            AccountMeta::new_readonly(source, false),      // 6: source
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // 7: source_token_account (unused)
            AccountMeta::new_readonly(spl_token_interface::ID, false), // 8: token_program (unused)
            AccountMeta::new(utils::MAGIC_VAULT, false),   // 9: magic_vault
            AccountMeta::new(magic_fee_vault, false),      // 10: magic_fee_vault
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false), // 11: magic_program
            AccountMeta::new(magic_context, false),        // 12: magic_context
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
    let (ctx, validator, mint, queue, receipt, vault, vault_token, source) =
        setup_context(receipt_data, group_id).await;

    // Simulate deposit_and_queue_transfer having already run (receipt is owned
    // by PROGRAM with splits set). Now shoot the callback directly.
    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        source,
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
    let header = bytemuck::try_from_bytes::<GroupReceiptHeader>(&account.data[..GroupReceiptHeader::SIZE]).unwrap();
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
    let (ctx, validator, mint, queue, receipt, vault, vault_token, source) =
        setup_context(receipt_data, group_id).await;

    let ix = callback_executor_ix(
        validator.pubkey(),
        receipt,
        queue,
        vault,
        mint,
        vault_token,
        source,
        true,
        200,
        group_id,
    );

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&validator.pubkey()), &[&validator], ctx.last_blockhash);

    let res = ctx.banks_client.process_transaction_with_metadata(tx).await.unwrap();
    res.result.unwrap();

    let callbacks = take_execute_callbacks();
    assert_eq!(callbacks.len(), 1);

    // No CreateEphemeralAccount — receipt was pre-initialized by deposit_and_queue_transfer.
    let creates = take_captured_ephemeral_creates(MAGIC_PROGRAM_ID);
    assert!(creates.is_empty(), "expected no CreateEphemeralAccount CPI");

    // CloseEphemeralAccount must have been called once — this was the last transfer.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(closes.len(), 1, "expected exactly one CloseEphemeralAccount CPI");
}

// ── group receipt permission ──────────────────────────────────────────────────

/// 15-account shape, last transfer: the receipt's ACL permission is closed
/// (rent refunded to the queue) alongside the receipt itself.
#[tokio::test]
#[serial]
async fn execute_callback_closes_receipt_permission_when_last_transfer() {
    let group_id: u32 = 1;
    let splits: u32 = 1;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (mut ctx, validator, mint, queue, receipt, vault, vault_token, source) =
        setup_context(receipt_data, group_id).await;
    acl_mock::clear_all_captured();

    // The permission close signs with the receipt PDA seeds, so the header
    // must carry the real bump (the shared fixture stores 0).
    let (_, receipt_bump) = utils::derive_group_receipt(queue, source, group_id);
    ctx.set_account(
        &receipt,
        &SolanaAccount {
            lamports: Rent::default().minimum_balance(GroupReceipt::required_size(splits as usize)),
            data: receipt_account_data(group_id, splits, receipt_bump),
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );

    let permission = utils::derive_group_receipt_permission(receipt);
    utils::pre_create_receipt_permission(&mut ctx, permission);

    let inner = append_permission_accounts(
        callback_ix(
            receipt,
            queue,
            vault,
            mint,
            vault_token,
            source,
            magic_fee_vault_pubkey(validator.pubkey()),
            MAGIC_CONTEXT_ID,
            true,
            200,
            group_id,
        ),
        permission,
    );
    let ix = callback_executor_ix_from(validator.pubkey(), inner);

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&validator.pubkey()), &[&validator], ctx.last_blockhash);
    let res = ctx.banks_client.process_transaction_with_metadata(tx).await.unwrap();
    res.result.unwrap();

    let permission_closes = acl_mock::take_captured_ephemeral_permission_closes();
    assert_eq!(
        permission_closes.len(),
        1,
        "expected exactly one CloseEphemeralPermission CPI"
    );
    let close = &permission_closes[0];
    assert_eq!(close.payer, queue, "rent must refund to the queue");
    assert_eq!(close.permissioned_account, receipt);
    assert_eq!(close.permission, permission);

    // The receipt itself must still be closed.
    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(closes.len(), 1, "expected exactly one CloseEphemeralAccount CPI");
}

/// 15-account shape, legacy receipt (created without a permission): the close
/// is skipped and the callback still completes.
#[tokio::test]
#[serial]
async fn execute_callback_skips_permission_close_for_legacy_receipt() {
    let group_id: u32 = 1;
    let splits: u32 = 1;

    let receipt_data = receipt_account_data(group_id, splits, 0);
    let (ctx, validator, mint, queue, receipt, vault, vault_token, source) =
        setup_context(receipt_data, group_id).await;
    acl_mock::clear_all_captured();
    // Permission account intentionally not created (data_len == 0).
    let permission = utils::derive_group_receipt_permission(receipt);

    let inner = append_permission_accounts(
        callback_ix(
            receipt,
            queue,
            vault,
            mint,
            vault_token,
            source,
            magic_fee_vault_pubkey(validator.pubkey()),
            MAGIC_CONTEXT_ID,
            true,
            200,
            group_id,
        ),
        permission,
    );
    let ix = callback_executor_ix_from(validator.pubkey(), inner);

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&validator.pubkey()), &[&validator], ctx.last_blockhash);
    let res = ctx.banks_client.process_transaction_with_metadata(tx).await.unwrap();
    res.result.unwrap();

    let permission_closes = acl_mock::take_captured_ephemeral_permission_closes();
    assert!(permission_closes.is_empty(), "expected no CloseEphemeralPermission CPI");

    let closes = take_captured_ephemeral_closes(MAGIC_PROGRAM_ID);
    assert_eq!(closes.len(), 1, "expected exactly one CloseEphemeralAccount CPI");
}
