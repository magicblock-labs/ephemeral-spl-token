mod utils;

use ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::{load_unchecked, RawType};
use ephemeral_spl_api::{program::ID, state::transfer_queue::TransferQueue};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::bpf_loader;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::utils::allocate_transfer_queue;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000;

#[tokio::test]
async fn test_allocate_transfer_queue() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);

    utils::add_associated_token_program(&mut pt);
    let data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        PERMISSION_PROGRAM_ID,
        solana_account::Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    pt.prefer_bpf(true);

    let mut context = pt.start_with_context().await;

    let payer_pubkey = context.payer.pubkey();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let (queue_pda, queue_bump) = TransferQueue::find_pda(&mint);

    // Setup mint/accounts via utils
    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer_pubkey,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let ix_allocate_queue = Instruction::new_with_bytes(
        PROGRAM,
        &vec![instruction::ALLOCATE_QUEUE],
        vec![
            AccountMeta::new(payer_pubkey, true),
            AccountMeta::new(queue_pda, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
    );

    let tx = Transaction::new_signed_with_payer(
        &[ix_allocate_queue],
        Some(&Pubkey::new_from_array(payer_pubkey.to_bytes())),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue_pda)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, PROGRAM);
    assert_eq!(queue_account.data.len(), 10240);
    let queue = unsafe { load_unchecked::<TransferQueue>(queue_account.data.as_slice()).unwrap() };
    assert_eq!(queue.mint, Pubkey::default());
    assert_eq!(queue.bump, queue_bump);
    assert_eq!(queue.length, 0);
}

#[tokio::test]
async fn test_allocate_full_transfer_queue() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);

    utils::add_associated_token_program(&mut pt);
    let data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        PERMISSION_PROGRAM_ID,
        solana_account::Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    pt.prefer_bpf(true);

    let mut context = pt.start_with_context().await;

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let (queue_pda, queue_bump) = TransferQueue::find_pda(&mint);

    allocate_transfer_queue(&mut context, mint, queue_pda).await;

    let queue_account = context
        .banks_client
        .get_account(queue_pda)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, PROGRAM);
    assert_eq!(queue_account.data.len(), TransferQueue::LEN);
    let queue = unsafe { load_unchecked::<TransferQueue>(queue_account.data.as_slice()).unwrap() };
    assert_eq!(queue.mint, Pubkey::default());
    assert_eq!(queue.bump, queue_bump);
    assert_eq!(queue.length, 0);
}
