use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::{load_unchecked, transfer_queue::TransferQueue, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::{allocate_transfer_queue, setup_program_test};

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000;

#[tokio::test]
async fn test_allocate_transfer_queue() {
    let pt = setup_program_test();

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
        ephemeral_spl_api::program::ID,
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

    assert_eq!(queue_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_account.data.len(), TransferQueue::LEN.min(10240));
    assert_eq!(queue_account.data[1..33], Pubkey::default().to_bytes());
    assert_eq!(queue_account.data[0], queue_bump);
}

#[tokio::test]
async fn test_allocate_full_transfer_queue() {
    let pt = setup_program_test();
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

    assert_eq!(queue_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_account.data.len(), TransferQueue::LEN);
    let queue = unsafe { load_unchecked::<TransferQueue>(queue_account.data.as_slice()).unwrap() };
    assert_eq!(queue.mint, Pubkey::default());
    assert_eq!(queue.bump, queue_bump);
    assert_eq!(queue.length, 0);
}
