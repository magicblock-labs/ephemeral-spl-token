use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::{
    state::transfer_queue::{HEADER_LEN, QUEUE_SEED, TRANSFER_QUEUE_VERSION},
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
use wheels::{layout::Encodable, variable_offset_layout};

use crate::common::{
    callback_mock,
    callback_mock::take_execute_callbacks,
    magic_mock,
    magic_mock::{take_captured_action_callbacks, take_captured_intent_bundles},
};

mod common;
mod utils;

// ── helpers ───────────────────────────────────────────────────────────────────

// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
struct RefundOnFailureArgs {
    pub amount: u64,
    pub retries_left: u8,
}

fn callback_ix_data(ok: bool, amount: u64, retries_left: u8) -> Vec<u8> {
    let args = RefundOnFailureArgs { amount, retries_left }.encode().unwrap();
    let mut data = Vec::new();
    data.push(internal::RefundOnFailureCallback.discriminator());
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
    // version(1) bump(1) _pad0(6) mint(32) length(4) _pad1(8) next_task_id(4) crank_task_id(8) validator(32)
    data[0] = TRANSFER_QUEUE_VERSION;
    data[1] = bump;
    data[8..40].copy_from_slice(&mint.to_bytes());
    data[64..96].copy_from_slice(&validator.to_bytes());
    data
}

/// Common fixture: ProgramTest context with queue, magic_fee_vault, and magic_context accounts.
/// Returns (context, validator, mint, queue, magic_fee_vault, refund_destination_owner).
async fn setup_context() -> (
    solana_program_test::ProgramTestContext,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let validator = Keypair::new();
    let mint = Keypair::new().pubkey();
    let refund_destination_owner = Keypair::new().pubkey();
    let (queue, queue_bump) = derive_queue(mint, validator.pubkey());
    let magic_fee_vault = utils::magic_fee_vault_pda(validator.pubkey());
    let magic_context = MAGIC_CONTEXT_ID;

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
        magic_fee_vault,
        SolanaAccount {
            lamports: 1_000_000,
            data: vec![],
            owner: utils::delegation_program_id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        magic_context,
        SolanaAccount {
            lamports: 1_000_000,
            data: vec![0u8; 8],
            owner: Pubkey::new_from_array(MAGIC_PROGRAM_ID.to_bytes()),
            executable: false,
            rent_epoch: 0,
        },
    );

    for pk in [mint, refund_destination_owner] {
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

    magic_mock::clear_all_captured(Pubkey::new_from_array(MAGIC_PROGRAM_ID.to_bytes()));

    (ctx, validator, mint, queue, magic_fee_vault, refund_destination_owner)
}

fn callback_ix(
    refund_destination_owner: Pubkey,
    queue: Pubkey,
    magic_fee_vault: Pubkey,
    ok: bool,
    amount: u64,
    retries_left: u8,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(CALLBACK_SIGNER, true),                    // 0: callback_signer
            AccountMeta::new_readonly(refund_destination_owner, false), // 1: refund_destination_owner
            AccountMeta::new(queue, false),                             // 2: queue_info
            AccountMeta::new(magic_fee_vault, false),                   // 3: magic_fee_vault
            AccountMeta::new(MAGIC_CONTEXT_ID, false),                  // 4: magic_context
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),         // 5: magic_program
        ],
        data: callback_ix_data(ok, amount, retries_left),
    }
}

fn callback_executor_ix(
    validator: Pubkey,
    refund_destination_owner: Pubkey,
    queue: Pubkey,
    magic_fee_vault: Pubkey,
    ok: bool,
    amount: u64,
    retries_left: u8,
) -> Instruction {
    let inner = callback_ix(
        refund_destination_owner,
        queue,
        magic_fee_vault,
        ok,
        amount,
        retries_left,
    );

    let mut account_metas = vec![
        AccountMeta::new_readonly(validator, true),
        AccountMeta::new_readonly(CALLBACK_SIGNER, false),
        AccountMeta::new_readonly(inner.program_id, false),
    ];
    account_metas.extend(inner.accounts.clone().into_iter().map(|mut el| {
        el.is_signer = false;
        el
    }));

    Instruction::new_with_bincode(
        CALLBACK_PROGRAM_ID,
        &CallbackInstruction::ExecuteCallback { instruction: inner },
        account_metas,
    )
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Success case (ok=true): callback fires, no intent bundle scheduled.
#[tokio::test]
#[serial]
async fn refund_on_failure_success_no_intent_bundle() {
    let (ctx, validator, _mint, queue, magic_fee_vault, refund_destination_owner) = setup_context().await;
    let magic_program = Pubkey::new_from_array(MAGIC_PROGRAM_ID.to_bytes());

    let ix = callback_executor_ix(
        validator.pubkey(),
        refund_destination_owner,
        queue,
        magic_fee_vault,
        true, // ok = true
        500,
        4,
    );

    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let callbacks = take_execute_callbacks();
    assert_eq!(callbacks.len(), 1);

    let bundles = take_captured_intent_bundles(magic_program);
    assert!(bundles.is_empty(), "expected no intent bundle on success");

    let action_callbacks = take_captured_action_callbacks(magic_program);
    assert!(action_callbacks.is_empty(), "expected no AddActionCallback on success");
}

/// Failure case (ok=false): callback fires, intent bundle with one refund action is scheduled,
/// and that action carries a RefundOnFailureCallback (discriminator 209).
#[tokio::test]
#[serial]
async fn refund_on_failure_schedules_intent_bundle() {
    let (ctx, validator, _mint, queue, magic_fee_vault, refund_destination_owner) = setup_context().await;
    let magic_program = Pubkey::new_from_array(MAGIC_PROGRAM_ID.to_bytes());
    const AMOUNT: u64 = 1_000;

    let ix = callback_executor_ix(
        validator.pubkey(),
        refund_destination_owner,
        queue,
        magic_fee_vault,
        false, // ok = false → triggers schedule_refund_on_failure
        AMOUNT,
        4,
    );

    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let callbacks = take_execute_callbacks();
    assert_eq!(callbacks.len(), 1);

    // One ScheduleIntentBundle CPI with one standalone action
    let bundles = take_captured_intent_bundles(magic_program);
    assert_eq!(bundles.len(), 1, "expected exactly one intent bundle");
    assert_eq!(
        bundles[0].args.standalone_actions.len(),
        1,
        "expected one standalone action in bundle"
    );

    let action = &bundles[0].args.standalone_actions[0];
    assert_eq!(
        action.destination_program.to_bytes(),
        PROGRAM.to_bytes(),
        "refund action destination must be PROGRAM"
    );
    assert_eq!(action.compute_units, 140_000);

    // One AddActionCallback CPI with RefundOnFailureCallback discriminator
    let action_callbacks = take_captured_action_callbacks(magic_program);
    assert_eq!(action_callbacks.len(), 1, "expected exactly one AddActionCallback");

    let cb = &action_callbacks[0].args;
    assert_eq!(cb.action_index, 0, "callback must target action index 0");
    assert_eq!(
        cb.destination_program.to_bytes(),
        PROGRAM.to_bytes(),
        "callback destination must be PROGRAM"
    );
    assert_eq!(
        cb.discriminator,
        vec![internal::RefundOnFailureCallback.discriminator()],
        "callback discriminator must be RefundOnFailureCallback (209)"
    );
    assert_eq!(cb.compute_units, 100_000);

    // Payload encodes amount (u64, 8 bytes) + retries_left (u8, 1 byte) = 9 bytes.
    // schedule_refund_on_failure decrements retries_left before encoding, so 4 → 3.
    assert_eq!(cb.payload.len(), 9, "payload must be 9 bytes");
    let decoded_amount = u64::from_le_bytes(cb.payload[0..8].try_into().expect("payload too short"));
    assert_eq!(decoded_amount, AMOUNT, "callback payload must encode AMOUNT");
    assert_eq!(cb.payload[8], 3, "retries_left must be decremented to 3");
}

/// Exhausted retries (ok=false, retries_left=0): callback returns RefundPermanentlyFailed,
/// no intent bundle is scheduled.
#[tokio::test]
#[serial]
async fn refund_on_failure_retries_exhausted() {
    let (ctx, validator, _mint, queue, magic_fee_vault, refund_destination_owner) = setup_context().await;
    let magic_program = Pubkey::new_from_array(MAGIC_PROGRAM_ID.to_bytes());
    const AMOUNT: u64 = 1_000;

    let ix = callback_executor_ix(
        validator.pubkey(),
        refund_destination_owner,
        queue,
        magic_fee_vault,
        false, // ok = false
        AMOUNT,
        0, // retries_left = 0 → permanent failure
    );

    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&validator.pubkey()),
        &[&validator],
        ctx.last_blockhash,
    );
    let result = ctx.banks_client.process_transaction(tx).await;
    assert!(result.is_err(), "expected transaction to fail on exhausted retries");

    let bundles = take_captured_intent_bundles(magic_program);
    assert!(bundles.is_empty(), "expected no intent bundle when retries exhausted");
}
