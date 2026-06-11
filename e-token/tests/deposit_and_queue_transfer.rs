use crate::utils::{pre_create_group_receipt, pre_create_stealth_pool};
use bytemuck::Zeroable;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::instructions::{DepositAndQueueTransferArgs, UpdateStealthPoolArgs};
use ephemeral_spl_api::state::group_receipt::GroupReceiptHeader;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::stealth_pool::{StealthPool, StealthPoolFlags};
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, QueuedTransfer, TransferQueue, TransferQueueHeader, HEADER_LEN, ITEM_LEN,
};
use ephemeral_spl_api::ID as PROGRAM;
use pinocchio::Address;
use serial_test::serial;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::clock::Clock;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use wheels::layout::Encodable as _;
use {
    solana_keypair::Keypair,
    solana_program_test::{processor, tokio, ProgramTestContext},
    solana_pubkey::{pubkey, Pubkey},
    solana_signer::Signer,
    solana_transaction::{InstructionError, Transaction, TransactionError},
};

const MAGIC_PROGRAM: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");

mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

struct Fixture {
    context: ProgramTestContext,
    payer_kp: Keypair,
    payer: Pubkey,
    mint: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    magic_vault: Pubkey,
    user_source_ata: Pubkey,
    destination_ata: Pubkey,
    vault_ata: Pubkey,
}

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= HEADER_LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

fn read_item_unaligned(data: &[u8], index: usize) -> QueuedTransfer {
    let offset = HEADER_LEN + (index * ITEM_LEN);
    assert!(data.len() >= offset + ITEM_LEN);
    unsafe { core::ptr::read_unaligned(data[offset..].as_ptr() as *const QueuedTransfer) }
}

async fn setup_fixture(items: Option<u32>) -> Fixture {
    common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.prefer_bpf(false);
        pt.add_program(
            "magic_mock",
            MAGIC_PROGRAM,
            processor!(common::magic_mock::process),
        );
        pt.prefer_bpf(true);
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair("deposit_and_queue_transfer::mint");
    let mint = mint_kp.pubkey();
    let validator = Keypair::new().pubkey();

    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        2,
    )
    .await;

    let (queue, _) = TransferQueue::find_pda(&mint, &validator);
    let vault = queue;
    let user_source_ata = setup.user_tokens[0];
    let destination_ata = utils::derive_associated_token_address(payer, mint);
    let vault_ata = utils::derive_associated_token_address(vault, mint);

    let ix_init_queue = utils::build_initialize_transfer_queue_ix(
        payer,
        queue,
        mint,
        validator,
        items,
        spl_token_interface::ID,
    );

    let ix_init_destination_ata = Instruction {
        program_id: utils::associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: vec![1],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_queue, ix_init_destination_ata],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    Fixture {
        context,
        payer_kp,
        payer,
        mint,
        queue,
        vault,
        magic_vault: utils::MAGIC_VAULT,
        user_source_ata,
        destination_ata,
        vault_ata,
    }
}

fn build_deposit_and_queue_ix(
    fixture: &Fixture,
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
    split: u32,
    group_id: u32,
    group_receipt: Pubkey,
) -> Instruction {
    build_deposit_and_queue_ix_with_options(
        fixture,
        amount,
        min_delay_ms,
        max_delay_ms,
        split,
        None,
        None,
        group_id,
        group_receipt,
    )
}

fn build_deposit_and_queue_ix_with_options(
    fixture: &Fixture,
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
    split: u32,
    flags: Option<u8>,
    client_ref_id: Option<u64>,
    group_id: u32,
    group_receipt: Pubkey,
) -> Instruction {
    build_deposit_and_queue_ix_for_destination(
        fixture,
        fixture.payer,
        amount,
        min_delay_ms,
        max_delay_ms,
        split,
        flags,
        client_ref_id,
        group_id,
        group_receipt,
    )
}

fn build_deposit_and_queue_ix_for_destination(
    fixture: &Fixture,
    destination: Pubkey,
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
    split: u32,
    flags: Option<u8>,
    client_ref_id: Option<u64>,
    group_id: u32,
    group_receipt: Pubkey,
) -> Instruction {
    let g = group_id.to_le_bytes();
    let data = instruction::ESplInstruction::DepositAndQueueTransfer.with_data(
        &DepositAndQueueTransferArgs {
            amount,
            group_id: [g[0], g[1], g[2]],
            min_delay_ms,
            max_delay_ms,
            split,
            flags,
            client_ref_id,
        }
        .encode()
        .unwrap(),
    );

    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.queue, false),           // 0: queue
            AccountMeta::new_readonly(fixture.vault, false),  // 1: vault
            AccountMeta::new_readonly(fixture.mint, false),   // 2: mint
            AccountMeta::new(fixture.user_source_ata, false), // 3: user_source_token
            AccountMeta::new(fixture.vault_ata, false),       // 4: vault_token
            AccountMeta::new_readonly(destination, false),    // 5: destination
            AccountMeta::new_readonly(fixture.payer, true),   // 6: user_authority (signer)
            AccountMeta::new_readonly(spl_token_interface::ID, false), // 7: token_program
            AccountMeta::new_readonly(PROGRAM, false),        // 8: reimbursement (placeholder)
            AccountMeta::new(group_receipt, false),           // 9: group_receipt_info
            AccountMeta::new(fixture.magic_vault, false),     // 10: magic_vault
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),  // 11: magic_program
        ],
        data,
    }
}

fn build_update_stealth_pool_ix(
    payer: Pubkey,
    authority: Pubkey,
    handle_hash: [u8; 32],
    flags: u8,
    destinations: &[Pubkey],
) -> (Pubkey, Instruction) {
    let (stealth_pool, _) = StealthPool::find_pda(&handle_hash);
    let destination_addresses = destinations
        .iter()
        .map(|destination| Address::new_from_array(destination.to_bytes()))
        .collect::<Vec<_>>();

    let data = instruction::ESplInstruction::UpdateStealthPool.with_data(
        &UpdateStealthPoolArgs {
            handle_hash,
            flags,
            destinations: destination_addresses,
        }
        .encode()
        .unwrap(),
    );

    (
        stealth_pool,
        Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(stealth_pool, false),
                AccountMeta::new_readonly(authority, true),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data,
        },
    )
}

fn expected_stealth_destination(
    handle_hash: &[u8; 32],
    destinations: &[Pubkey],
    split_across_keys: bool,
    source: &Pubkey,
    group_id: u32,
    first_queue_position: usize,
    queue_position: usize,
    client_ref_id: u64,
    split_index: usize,
) -> Pubkey {
    let queue_seed = if split_across_keys {
        queue_position
    } else {
        first_queue_position
    };
    let split_seed = if split_across_keys { split_index } else { 0 };
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for byte in handle_hash {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    for byte in source.to_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    for byte in group_id.to_le_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    for byte in (queue_seed as u64).to_le_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    for byte in client_ref_id.to_le_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    for byte in (split_seed as u64).to_le_bytes() {
        value ^= u64::from(byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    value ^= value >> 32;
    destinations[(value % destinations.len() as u64) as usize]
}

fn expected_split_delay_ms(
    destination: &Pubkey,
    queue_position: usize,
    min_delay_ms: u64,
    max_delay_ms: u64,
) -> u64 {
    if min_delay_ms == max_delay_ms {
        return min_delay_ms;
    }

    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in destination.to_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash ^= queue_position as u64;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    let hash = hash ^ (hash >> 32);
    let sample_space = max_delay_ms - min_delay_ms + 1;
    min_delay_ms + (hash % sample_space)
}

async fn assert_empty_state(fixture: &Fixture) {
    let user_token_acc = fixture
        .context
        .banks_client
        .get_account(fixture.user_source_ata)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state = Account::unpack(&user_token_acc.data).unwrap();
    assert_eq!(user_token_state.amount, STARTING_BALANCE);

    let vault_token_acc = fixture
        .context
        .banks_client
        .get_account(fixture.vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist");
    let vault_token_state = Account::unpack(&vault_token_acc.data).unwrap();
    assert_eq!(vault_token_state.amount, 0);

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");
    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.length, 0);
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_transfers_once_and_enqueues_split_items() {
    let mut fixture = setup_fixture(None).await;
    let clock_before = fixture
        .context
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap();

    let amount: u64 = 10;
    let min_delay_ms: u64 = 120_000;
    let max_delay_ms: u64 = 120_000;
    let split: u32 = 3;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let ix = build_deposit_and_queue_ix(
        &fixture,
        amount,
        min_delay_ms,
        max_delay_ms,
        split,
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::once_split",
    )
    .await
    .unwrap();

    let clock_after = fixture
        .context
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap();

    let user_token_acc_after = fixture
        .context
        .banks_client
        .get_account(fixture.user_source_ata)
        .await
        .unwrap()
        .expect("user token account must exist after deposit");
    let user_token_state_after = Account::unpack(&user_token_acc_after.data).unwrap();
    assert_eq!(user_token_state_after.amount, STARTING_BALANCE - amount);

    let vault_token_acc_after = fixture
        .context
        .banks_client
        .get_account(fixture.vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist after deposit");
    let vault_token_state_after = Account::unpack(&vault_token_acc_after.data).unwrap();
    assert_eq!(vault_token_state_after.amount, amount);

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.length, split);

    let mut queued_amounts = [0u64; 3];
    let mut shared_group_id = None;
    let mut shared_client_ref_id = None;
    for (index, queued_amount) in queued_amounts.iter_mut().enumerate().take(split as usize) {
        let queued = read_item_unaligned(&queue_account.data, index);
        *queued_amount = queued.amount;
        assert_eq!(queued.source.as_array(), &fixture.payer.to_bytes());
        assert_eq!(
            queued.destination_owner.as_array(),
            &fixture.payer.to_bytes()
        );
        let group_id = queued.group_id();
        assert!(group_id != 0);
        if let Some(expected_group_id) = shared_group_id {
            assert_eq!(group_id, expected_group_id);
        } else {
            shared_group_id = Some(group_id);
        }
        let implied_now_ms = (queued.ready_at - min_delay_ms as i64) as u64;
        let client_ref_id = queued.client_ref_id;
        assert_eq!(client_ref_id, 0);
        if let Some(expected_client_ref_id) = shared_client_ref_id {
            assert_eq!(client_ref_id, expected_client_ref_id);
        } else {
            shared_client_ref_id = Some(client_ref_id);
        }
        assert_eq!(
            queued.flags,
            ephemeral_spl_api::state::transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA
        );
        assert!(implied_now_ms >= (clock_before.unix_timestamp * 1_000) as u64);
        assert!(implied_now_ms <= (clock_after.unix_timestamp * 1_000) as u64);
    }

    queued_amounts.sort_unstable();
    assert_eq!(queued_amounts, [3, 3, 4]);

    // One CreateEphemeralAccount CPI must have been sent to the magic program.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(
        creates.len(),
        1,
        "expected exactly one CreateEphemeralAccount CPI"
    );

    // Receipt must be initialized with the correct header (not yet used, so transfers_completed=0).
    let receipt_acc = fixture
        .context
        .banks_client
        .get_account(group_receipt)
        .await
        .unwrap()
        .expect("group receipt account must exist");
    let receipt_header = bytemuck::try_from_bytes::<GroupReceiptHeader>(
        &receipt_acc.data[..GroupReceiptHeader::SIZE],
    )
    .unwrap();
    assert_eq!(receipt_header.id(), group_id);
    assert_eq!(receipt_header.splits(), split);
    assert_eq!(receipt_header.transfer_completed(), 0);
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_assigns_distinct_group_ids_per_enqueue() {
    let mut fixture = setup_fixture(None).await;
    let receipt1 =
        utils::pre_create_group_receipt(&mut fixture.context, fixture.queue, fixture.payer, 1, 2);
    let receipt2 =
        utils::pre_create_group_receipt(&mut fixture.context, fixture.queue, fixture.payer, 2, 3);
    let first_ix = build_deposit_and_queue_ix(&fixture, 10, 0, 0, 2, 1, receipt1);
    let second_ix = build_deposit_and_queue_ix(&fixture, 12, 0, 0, 3, 2, receipt2);

    for (i, ix) in [first_ix, second_ix].into_iter().enumerate() {
        let blockhash = fixture
            .context
            .banks_client
            .get_latest_blockhash()
            .await
            .unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.payer),
            &[&fixture.payer_kp],
            blockhash,
        );
        common::metrics::process_transaction_record_cu(
            &fixture.context.banks_client,
            tx,
            &format!("dep_queue::assign_group_id_{}", i),
        )
        .await
        .unwrap();
    }

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");
    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.length, 5);

    let mut group_ids = (0..header.length as usize)
        .map(|index| read_item_unaligned(&queue_account.data, index).group_id())
        .collect::<Vec<_>>();
    group_ids.sort_unstable();
    assert!(group_ids[0] != 0);
    assert_eq!(group_ids[0], group_ids[1]);
    assert_eq!(group_ids[2], group_ids[3]);
    assert_eq!(group_ids[3], group_ids[4]);
    assert!(group_ids[1] != group_ids[2]);

    // Two enqueues = two CreateEphemeralAccount CPIs (accumulated across both transactions).
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 2, "expected two CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_uses_explicit_client_ref_id_for_all_splits() {
    let mut fixture = setup_fixture(None).await;
    let client_ref_id = 0x1234_5678_9abc_def0_u64;
    let split = 3;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let ix = build_deposit_and_queue_ix_with_options(
        &fixture,
        12,
        0,
        0,
        split,
        None,
        Some(client_ref_id),
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::all_splits_use_client_ref_id",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    for index in 0..split as usize {
        let queued = read_item_unaligned(&queue_account.data, index);
        assert_eq!(queued.client_ref_id, client_ref_id);
    }

    // One enqueue = one CreateEphemeralAccount CPI.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(
        creates.len(),
        1,
        "expected exactly one CreateEphemeralAccount CPI"
    );
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_accepts_legacy_destination_ata() {
    let mut fixture = setup_fixture(None).await;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        1,
    );
    let ix = build_deposit_and_queue_ix_for_destination(
        &fixture,
        fixture.destination_ata,
        10,
        0,
        0,
        1,
        None,
        None,
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "dep_queue::rejects_legacy_destination_ata",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
    );

    // Transaction rejected before CPI — no CreateEphemeralAccount CPIs expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");

    assert_empty_state(&fixture).await;
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_rejects_zero_split() {
    let fixture = setup_fixture(None).await;
    let (group_receipt, _) = utils::derive_group_receipt(fixture.queue, fixture.payer, 1);
    let ix = build_deposit_and_queue_ix(&fixture, 10, 0, 0, 0, 1, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "dep_queue::reject_zero_split",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;

    // Transaction rejected before CPI — no CreateEphemeralAccount CPIs expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_rejects_split_greater_than_amount() {
    let fixture = setup_fixture(None).await;
    let (group_receipt, _) = utils::derive_group_receipt(fixture.queue, fixture.payer, 1);
    let ix = build_deposit_and_queue_ix(&fixture, 2, 0, 0, 3, 1, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "dep_queue::reject_split_gt_amt",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;

    // Transaction rejected before CPI — no CreateEphemeralAccount CPIs expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_rejects_when_queue_is_full() {
    let items = 2;
    let fixture = setup_fixture(Some(items)).await;
    let (group_receipt, _) = utils::derive_group_receipt(fixture.queue, fixture.payer, 1);
    let ix = build_deposit_and_queue_ix(&fixture, 6, 0, 0, 3, 1, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "dep_queue::reject_queue_full",
    )
    .await
    .unwrap()
    .result
    .unwrap();

    assert_empty_state(&fixture).await;

    // Queue full — rejected before group-receipt CPI, no CreateEphemeralAccount CPIs expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_rejects_invalid_delay_range() {
    let fixture = setup_fixture(None).await;
    let (group_receipt, _) = utils::derive_group_receipt(fixture.queue, fixture.payer, 1);
    let ix = build_deposit_and_queue_ix(&fixture, 10, 10, 9, 1, 1, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "dep_queue::reject_delay_range",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;

    // Transaction rejected before CPI — no CreateEphemeralAccount CPIs expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_uses_deterministic_split_delays_within_range() {
    let mut fixture = setup_fixture(None).await;
    let amount: u64 = 12;
    let min_delay_ms: u64 = 100;
    let max_delay_ms: u64 = 300;
    let split: u32 = 4;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let ix = build_deposit_and_queue_ix(
        &fixture,
        amount,
        min_delay_ms,
        max_delay_ms,
        split,
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::deterministic_delays",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let mut actual_ready_ats = Vec::new();
    for index in 0..split as usize {
        let queued = read_item_unaligned(&queue_account.data, index);
        actual_ready_ats.push(queued.ready_at);
    }

    let mut expected_delays = (0..split as usize)
        .map(|index| expected_split_delay_ms(&fixture.payer, index, min_delay_ms, max_delay_ms))
        .collect::<Vec<_>>();

    actual_ready_ats.sort_unstable();
    expected_delays.sort_unstable();
    let implied_now_ms = actual_ready_ats[0] - expected_delays[0] as i64;
    for (ready_at, expected_delay_ms) in actual_ready_ats.iter().zip(expected_delays.iter()) {
        assert_eq!(*ready_at - *expected_delay_ms as i64, implied_now_ms);
    }

    // One CreateEphemeralAccount CPI must have been sent to the magic program.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(
        creates.len(),
        1,
        "expected exactly one CreateEphemeralAccount CPI"
    );
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_prefers_multiples_of_five_for_four_way_split() {
    let mut fixture = setup_fixture(None).await;
    let amount: u64 = 33_500_000;
    let split: u32 = 4;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let ix = build_deposit_and_queue_ix(&fixture, amount, 0, 0, split, group_id, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::split4_mod5",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let mut queued_amounts = (0..split as usize)
        .map(|index| read_item_unaligned(&queue_account.data, index).amount)
        .collect::<Vec<_>>();
    queued_amounts.sort_unstable();
    assert_eq!(
        queued_amounts,
        vec![3_500_000, 10_000_000, 10_000_000, 10_000_000]
    );

    // One CreateEphemeralAccount CPI must have been sent to the magic program.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(
        creates.len(),
        1,
        "expected exactly one CreateEphemeralAccount CPI"
    );
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_prefers_multiples_of_five_for_three_way_split() {
    let mut fixture = setup_fixture(None).await;
    let amount: u64 = 33_500_000;
    let split: u32 = 3;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let ix = build_deposit_and_queue_ix(&fixture, amount, 0, 0, split, group_id, group_receipt);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::split3_mod5",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let mut queued_amounts = (0..split as usize)
        .map(|index| read_item_unaligned(&queue_account.data, index).amount)
        .collect::<Vec<_>>();
    queued_amounts.sort_unstable();
    assert_eq!(queued_amounts, vec![3_500_000, 15_000_000, 15_000_000]);

    // One CreateEphemeralAccount CPI must have been sent to the magic program.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(
        creates.len(),
        1,
        "expected exactly one CreateEphemeralAccount CPI"
    );
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_return_to_shuttle() {
    let fixture = setup_fixture(None).await;

    let shuttle_id = 42_u32;
    let (shuttle_ephemeral_ata, _) =
        ShuttleMetadata::find_pda(&fixture.payer, &fixture.mint, shuttle_id);
    let shuttle_wallet_ata =
        utils::derive_associated_token_address(shuttle_ephemeral_ata, fixture.mint);
    let ix_init_ata = Instruction {
        program_id: utils::associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(fixture.payer, true),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: vec![1],
    };

    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();
    let tx_init_ata = Transaction::new_signed_with_payer(
        &[ix_init_ata],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx_init_ata)
        .await
        .unwrap();

    let amount: u64 = 33_500_000;
    let split: u32 = 1000;
    let group_id: u32 = 1;
    let (group_receipt_pda, _) =
        utils::derive_group_receipt(fixture.queue, fixture.payer, group_id);
    let ix = {
        let g = group_id.to_le_bytes();
        let data = instruction::ESplInstruction::DepositAndQueueTransfer.with_data(
            &DepositAndQueueTransferArgs {
                amount,
                group_id: [g[0], g[1], g[2]],
                min_delay_ms: 0,
                max_delay_ms: 0,
                split,
                flags: None,
                client_ref_id: None,
            }
            .encode()
            .unwrap(),
        );

        Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(fixture.queue, false),           // 0: queue
                AccountMeta::new_readonly(fixture.vault, false),  // 1: vault
                AccountMeta::new_readonly(fixture.mint, false),   // 2: mint
                AccountMeta::new(fixture.user_source_ata, false), // 3: user_source_token
                AccountMeta::new(fixture.vault_ata, false),       // 4: vault_token
                AccountMeta::new_readonly(fixture.payer, false),  // 5: destination
                AccountMeta::new_readonly(fixture.payer, true),   // 6: user_authority (signer)
                AccountMeta::new_readonly(spl_token_interface::ID, false), // 7: token_program
                AccountMeta::new(shuttle_wallet_ata, false),      // 8: reimbursement
                AccountMeta::new(group_receipt_pda, false),       // 9: group_receipt_info
                AccountMeta::new(fixture.magic_vault, false),     // 10: magic_vault
                AccountMeta::new_readonly(MAGIC_PROGRAM, false),  // 11: magic_program
            ],
            data,
        }
    };
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::return_to_shuttle",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let (header, items) = queue_views_checked(&queue_account.data).unwrap();
    assert_eq!(header.length, 0);
    assert!(items.iter().all(|item| item == &QueuedTransfer::zeroed()));

    let shuttle_token_acc = fixture
        .context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle token account must exist");
    let shuttle_token_state = Account::unpack(&shuttle_token_acc.data).unwrap();
    assert_eq!(shuttle_token_state.amount, amount);

    // return_to_shuttle takes a different code path — no group-receipt CPI expected.
    let creates = common::magic_mock::take_captured_ephemeral_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 0, "expected no CreateEphemeralAccount CPIs");
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_resolves_stealth_pool_destination() {
    let mut fixture = setup_fixture(None).await;
    let handle_hash = [42u8; 32];
    let destinations = [
        utils::test_keypair("stealth_pool::destination_0").pubkey(),
        utils::test_keypair("stealth_pool::destination_1").pubkey(),
    ];
    let (stealth_pool, init_ix) = build_update_stealth_pool_ix(
        fixture.payer,
        fixture.payer,
        handle_hash,
        StealthPoolFlags::Empty.value(),
        &destinations,
    );
    pre_create_stealth_pool(&mut fixture.context, stealth_pool);
    let split = 3;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let deposit_ix = build_deposit_and_queue_ix_for_destination(
        &fixture,
        stealth_pool,
        12,
        0,
        0,
        split,
        None,
        None,
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[init_ix, deposit_ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::stealth_pool_resolve",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");
    let expected = expected_stealth_destination(
        &handle_hash,
        &destinations,
        false,
        &fixture.payer,
        1,
        0,
        0,
        0,
        0,
    );

    for index in 0..split as usize {
        let queued = read_item_unaligned(&queue_account.data, index);
        assert_eq!(queued.destination_owner.as_array(), &expected.to_bytes());
        assert_ne!(
            queued.destination_owner.as_array(),
            &stealth_pool.to_bytes()
        );
    }
}

#[tokio::test]
#[serial]
async fn deposit_and_queue_transfer_can_split_stealth_pool_across_keys() {
    let mut fixture = setup_fixture(None).await;
    let handle_hash = [77u8; 32];
    let destinations = [
        utils::test_keypair("stealth_pool::split_destination_0").pubkey(),
        utils::test_keypair("stealth_pool::split_destination_1").pubkey(),
        utils::test_keypair("stealth_pool::split_destination_2").pubkey(),
    ];
    let (stealth_pool, init_ix) = build_update_stealth_pool_ix(
        fixture.payer,
        fixture.payer,
        handle_hash,
        StealthPoolFlags::SplitAcrossKeys.value(),
        &destinations,
    );
    pre_create_stealth_pool(&mut fixture.context, stealth_pool);
    let split = 5;
    let client_ref_id = 99;
    let group_id: u32 = 1;
    let group_receipt = pre_create_group_receipt(
        &mut fixture.context,
        fixture.queue,
        fixture.payer,
        group_id,
        split,
    );
    let deposit_ix = build_deposit_and_queue_ix_for_destination(
        &fixture,
        stealth_pool,
        15,
        0,
        0,
        split,
        None,
        Some(client_ref_id),
        group_id,
        group_receipt,
    );
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[init_ix, deposit_ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "dep_queue::stealth_pool_split_keys",
    )
    .await
    .unwrap();

    let queue_account = fixture
        .context
        .banks_client
        .get_account(fixture.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    let mut actual = (0..split as usize)
        .map(|index| {
            read_item_unaligned(&queue_account.data, index)
                .destination_owner
                .to_bytes()
        })
        .collect::<Vec<_>>();
    let mut expected = (0..split as usize)
        .map(|index| {
            expected_stealth_destination(
                &handle_hash,
                &destinations,
                true,
                &fixture.payer,
                1,
                0,
                index,
                client_ref_id,
                index,
            )
            .to_bytes()
        })
        .collect::<Vec<_>>();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}
