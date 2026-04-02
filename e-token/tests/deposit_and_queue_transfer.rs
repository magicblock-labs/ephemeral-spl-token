use bytemuck::Zeroable;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    header_len, item_len, queue_views_checked, QueuedTransfer, TransferQueueHeader, QUEUE_SEED,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::clock::Clock;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {
    solana_program_test::{tokio, ProgramTest, ProgramTestContext},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::{InstructionError, Transaction, TransactionError},
};

mod utils;

use crate::utils::add_permission_program;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

struct Fixture {
    context: ProgramTestContext,
    payer: Pubkey,
    mint: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    user_source_ata: Pubkey,
    destination_ata: Pubkey,
    vault_ata: Pubkey,
}

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= header_len());
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

fn read_item_unaligned(data: &[u8], index: usize) -> QueuedTransfer {
    let offset = header_len() + (index * item_len());
    assert!(data.len() >= offset + item_len());
    unsafe { core::ptr::read_unaligned(data[offset..].as_ptr() as *const QueuedTransfer) }
}

async fn setup_fixture(items: Option<u32>) -> Fixture {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    add_permission_program(&mut pt);
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();
    let validator = Keypair::new().pubkey();

    let pdas = utils::derive_pdas(PROGRAM, payer, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        2,
    )
    .await;

    let queue =
        Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref(), validator.as_ref()], &PROGRAM).0;
    let queue_permission = permission_pda_from_permissioned_account(&queue);
    let vault = pdas.vault;
    let user_source_ata = setup.user_tokens[0];
    let destination_ata = utils::derive_associated_token_address(payer, mint);
    let (vault_eata, _) = Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
    let vault_ata = utils::derive_associated_token_address(vault, mint);

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
    };

    let mut queue_init_data = vec![instruction::INITIALIZE_TRANSFER_QUEUE];
    if let Some(items) = items {
        queue_init_data.extend_from_slice(&items.to_le_bytes());
    }
    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(queue_permission, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: queue_init_data,
    };

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
        &[ix_init_vault, ix_init_queue, ix_init_destination_ata],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    Fixture {
        context,
        payer,
        mint,
        queue,
        vault,
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
) -> Instruction {
    build_deposit_and_queue_ix_with_options(
        fixture,
        amount,
        min_delay_ms,
        max_delay_ms,
        split,
        None,
        None,
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
) -> Instruction {
    let mut data = vec![instruction::DEPOSIT_AND_QUEUE_TRANSFER];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&min_delay_ms.to_le_bytes());
    data.extend_from_slice(&max_delay_ms.to_le_bytes());
    data.extend_from_slice(&split.to_le_bytes());
    if let Some(flags) = flags {
        data.push(flags);
    }
    if let Some(client_ref_id) = client_ref_id {
        data.extend_from_slice(&client_ref_id.to_le_bytes());
    }

    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new_readonly(fixture.vault, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new(fixture.user_source_ata, false),
            AccountMeta::new(fixture.vault_ata, false),
            AccountMeta::new_readonly(fixture.destination_ata, false),
            AccountMeta::new_readonly(fixture.payer, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(PROGRAM, false),
        ],
        data,
    }
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
async fn deposit_and_queue_transfer_transfers_once_and_enqueues_split_items() {
    let fixture = setup_fixture(None).await;
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
    let ix = build_deposit_and_queue_ix(&fixture, amount, min_delay_ms, max_delay_ms, split);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
        assert_eq!(queued.flags, 0);
        assert!(implied_now_ms >= (clock_before.unix_timestamp * 1_000) as u64);
        assert!(implied_now_ms <= (clock_after.unix_timestamp * 1_000) as u64);
    }

    queued_amounts.sort_unstable();
    assert_eq!(queued_amounts, [3, 3, 4]);
}

#[tokio::test]
async fn deposit_and_queue_transfer_assigns_distinct_group_ids_per_enqueue() {
    let fixture = setup_fixture(None).await;
    let first_ix = build_deposit_and_queue_ix(&fixture, 10, 0, 0, 2);
    let second_ix = build_deposit_and_queue_ix(&fixture, 12, 0, 0, 3);

    for ix in [first_ix, second_ix] {
        let blockhash = fixture
            .context
            .banks_client
            .get_latest_blockhash()
            .await
            .unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.payer),
            &[&fixture.context.payer],
            blockhash,
        );
        fixture
            .context
            .banks_client
            .process_transaction(tx)
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
}

#[tokio::test]
async fn deposit_and_queue_transfer_uses_explicit_client_ref_id_for_all_splits() {
    let fixture = setup_fixture(None).await;
    let client_ref_id = 0x1234_5678_9abc_def0_u64;
    let split = 3;
    let ix = build_deposit_and_queue_ix_with_options(
        &fixture,
        12,
        0,
        0,
        split,
        None,
        Some(client_ref_id),
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
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
}

#[tokio::test]
async fn deposit_and_queue_transfer_rejects_zero_split() {
    let fixture = setup_fixture(None).await;
    let ix = build_deposit_and_queue_ix(&fixture, 10, 0, 0, 0);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    assert_eq!(
        fixture
            .context
            .banks_client
            .process_transaction(tx)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;
}

#[tokio::test]
async fn deposit_and_queue_transfer_rejects_split_greater_than_amount() {
    let fixture = setup_fixture(None).await;
    let ix = build_deposit_and_queue_ix(&fixture, 2, 0, 0, 3);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    assert_eq!(
        fixture
            .context
            .banks_client
            .process_transaction(tx)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;
}

#[tokio::test]
async fn deposit_and_queue_transfer_rejects_when_queue_is_full() {
    let items = 2;
    let fixture = setup_fixture(Some(items)).await;
    let ix = build_deposit_and_queue_ix(&fixture, 6, 0, 0, 3);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    assert_empty_state(&fixture).await;
}

#[tokio::test]
async fn deposit_and_queue_transfer_rejects_invalid_delay_range() {
    let fixture = setup_fixture(None).await;
    let ix = build_deposit_and_queue_ix(&fixture, 10, 10, 9, 1);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    assert_eq!(
        fixture
            .context
            .banks_client
            .process_transaction(tx)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    assert_empty_state(&fixture).await;
}

#[tokio::test]
async fn deposit_and_queue_transfer_uses_deterministic_split_delays_within_range() {
    let fixture = setup_fixture(None).await;
    let amount: u64 = 12;
    let min_delay_ms: u64 = 100;
    let max_delay_ms: u64 = 300;
    let split: u32 = 4;
    let ix = build_deposit_and_queue_ix(&fixture, amount, min_delay_ms, max_delay_ms, split);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
}

#[tokio::test]
async fn deposit_and_queue_transfer_prefers_multiples_of_five_for_four_way_split() {
    let fixture = setup_fixture(None).await;
    let amount: u64 = 33_500_000;
    let split: u32 = 4;
    let ix = build_deposit_and_queue_ix(&fixture, amount, 0, 0, split);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
}

#[tokio::test]
async fn deposit_and_queue_transfer_prefers_multiples_of_five_for_three_way_split() {
    let fixture = setup_fixture(None).await;
    let amount: u64 = 33_500_000;
    let split: u32 = 3;
    let ix = build_deposit_and_queue_ix(&fixture, amount, 0, 0, split);
    let blockhash = fixture
        .context
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
}

#[tokio::test]
async fn deposit_and_queue_transfer_return_to_shuttle() {
    let fixture = setup_fixture(None).await;

    let shuttle_id = 42_u32;
    let (shuttle_ephemeral_ata, _) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, fixture.payer, fixture.mint, shuttle_id);
    let (_shuttle_eata, _) =
        utils::derive_shuttle_eata(PROGRAM, shuttle_ephemeral_ata, fixture.mint);
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
        &[&fixture.context.payer],
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
    let ix = {
        let mut data = vec![instruction::DEPOSIT_AND_QUEUE_TRANSFER];
        data.extend_from_slice(&amount.to_le_bytes());
        data.extend_from_slice(&0_u64.to_le_bytes());
        data.extend_from_slice(&0_u64.to_le_bytes());
        data.extend_from_slice(&split.to_le_bytes());

        Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(fixture.queue, false),
                AccountMeta::new_readonly(fixture.vault, false),
                AccountMeta::new_readonly(fixture.mint, false),
                AccountMeta::new(fixture.user_source_ata, false),
                AccountMeta::new(fixture.vault_ata, false),
                AccountMeta::new_readonly(fixture.destination_ata, false),
                AccountMeta::new_readonly(fixture.payer, true),
                AccountMeta::new_readonly(spl_token_interface::ID, false),
                AccountMeta::new(shuttle_wallet_ata, false),
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
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
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
}
