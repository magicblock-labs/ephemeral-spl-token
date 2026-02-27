use crate::utils::initialize_transfer_queue;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::transfer_queue::{QueuedTransfer, TransferQueue};
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;

use {solana_program_test::tokio, solana_signer::Signer, solana_transaction::Transaction};

mod utils;

use crate::utils::setup_program_test;

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
async fn queue_transfer_transfers_tokens_from_user_to_queue() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive PDAs and setup mint/accounts via utils
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let user_ata = setup.user_tokens[0];
    let recipient_ata = setup.recipient_tokens[0];
    let queue_shuttle_id = 0;
    let pdas = utils::derive_pdas(
        ephemeral_spl_api::program::ID,
        payer,
        mint,
        queue_shuttle_id,
    );

    // Assert initial SPL token balances
    let user_token_acc_before = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state_before = Account::unpack(&user_token_acc_before.data).unwrap();
    assert_eq!(user_token_state_before.amount, STARTING_BALANCE);

    initialize_transfer_queue(&mut context, payer, mint, queue_shuttle_id, &pdas).await;

    let depositor_ata_before = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("depositor ata account must exist");
    let depositor_ata_state_before = Account::unpack(&depositor_ata_before.data).unwrap();
    assert_eq!(depositor_ata_state_before.amount, STARTING_BALANCE);

    let queue_ata_before = context
        .banks_client
        .get_account(pdas.queue_ata)
        .await
        .unwrap()
        .expect("queue ata account must exist");
    let queue_ata_state_before = Account::unpack(&queue_ata_before.data).unwrap();
    assert_eq!(queue_ata_state_before.amount, 0);

    // Queue transfer
    let amount: u64 = 100 * 10u64.pow(DECIMALS as u32);
    let chunk_size: u64 = 10 * 10u64.pow(DECIMALS as u32);
    let interval_seconds: u16 = 10;
    let mut data = vec![instruction::QUEUE_TRANSFER];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&chunk_size.to_le_bytes());
    data.extend_from_slice(&interval_seconds.to_le_bytes());

    let ix_queue_transfer = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(mint, false), // [] Mint pubkey (seed/consistency)
            AccountMeta::new_readonly(user, true),  // [signer] user source token acc
            AccountMeta::new(user_ata, false),      // [writable] user source token acc
            AccountMeta::new(pdas.queue, false),    // [writable] queue token acc
            AccountMeta::new(pdas.queue_ata, false), // [writable] queue token acc
            AccountMeta::new_readonly(recipient_ata, false), // [] recipient token acc
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program id (readonly)
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_queue_transfer],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Assert SPL token balances after deposit
    let user_token_acc_after = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist after deposit");
    let user_token_state_after = Account::unpack(&user_token_acc_after.data).unwrap();
    assert_eq!(user_token_state_after.amount, STARTING_BALANCE - amount);

    let queue_token_acc_after = context
        .banks_client
        .get_account(pdas.queue_ata)
        .await
        .unwrap()
        .expect("queue token account must exist after deposit");
    let queue_token_state_after = Account::unpack(&queue_token_acc_after.data).unwrap();
    assert_eq!(queue_token_state_after.amount, amount);

    // Read back the Ephemeral ATA and verify amount incremented
    let account = context
        .banks_client
        .get_account(pdas.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(account.data.len(), TransferQueue::LEN);

    let mut mut_acc = account.data.clone();
    let queue_data =
        unsafe { load_mut_unchecked::<TransferQueue>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(queue_data.length, 1);
    let expected_transfer = QueuedTransfer {
        amount,
        chunk_size,
        interval_seconds,
        source: user_ata,
        destination: recipient_ata,
        last_transfer: queue_data.queue[0].last_transfer,
    };
    assert_eq!(queue_data.queue[0], expected_transfer);
}
