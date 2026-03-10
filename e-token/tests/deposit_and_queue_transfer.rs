use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    header_len, item_len, QueuedTransfer, TransferQueueHeader, QUEUE_SEED,
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

async fn setup_fixture(queue_size_bytes: Option<u32>) -> Fixture {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

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

    let queue = Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref()], &PROGRAM).0;
    let vault = pdas.vault;
    let vault_bump = pdas.bump_vault;
    let user_source_ata = setup.user_tokens[0];
    let destination_ata = setup.user_tokens[1];
    let (vault_eata, _vault_eata_bump) =
        Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
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
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, vault_bump],
    };

    let mut queue_init_data = vec![instruction::INITIALIZE_TRANSFER_QUEUE];
    if let Some(queue_size_bytes) = queue_size_bytes {
        queue_init_data.extend_from_slice(&queue_size_bytes.to_le_bytes());
    }
    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: queue_init_data,
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_vault, ix_init_queue],
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
    delay_seconds: u64,
    split: u32,
) -> Instruction {
    let mut data = vec![instruction::DEPOSIT_AND_QUEUE_TRANSFER];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&delay_seconds.to_le_bytes());
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
        ],
        data,
    }
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
    let delay_seconds: u64 = 120;
    let split: u32 = 3;
    let ix = build_deposit_and_queue_ix(&fixture, amount, delay_seconds, split);
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
    for (index, queued_amount) in queued_amounts.iter_mut().enumerate().take(split as usize) {
        let queued = read_item_unaligned(&queue_account.data, index);
        *queued_amount = queued.amount;
        assert_eq!(queued.source.as_array(), &fixture.payer.to_bytes());
        assert_eq!(
            queued.destination.as_array(),
            &fixture.destination_ata.to_bytes()
        );
        assert_eq!(queued.ready_at - queued.inserted_at, delay_seconds as i64);
        assert!(queued.inserted_at >= clock_before.unix_timestamp);
        assert!(queued.inserted_at <= clock_after.unix_timestamp);
    }

    queued_amounts.sort_unstable();
    assert_eq!(queued_amounts, [3, 3, 4]);
}

#[tokio::test]
async fn deposit_and_queue_transfer_rejects_zero_split() {
    let fixture = setup_fixture(None).await;
    let ix = build_deposit_and_queue_ix(&fixture, 10, 0, 0);
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
    let ix = build_deposit_and_queue_ix(&fixture, 2, 0, 3);
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
    let queue_size_bytes = (header_len() + (item_len() * 2)) as u32;
    let fixture = setup_fixture(Some(queue_size_bytes)).await;
    let ix = build_deposit_and_queue_ix(&fixture, 6, 0, 3);
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
        TransactionError::InstructionError(0, InstructionError::AccountDataTooSmall)
    );

    assert_empty_state(&fixture).await;
}
