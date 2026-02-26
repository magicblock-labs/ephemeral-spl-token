use crate::utils::associated_token_program_id;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::transfer_queue::{QueuedTransfer, TransferQueue};
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;

use {
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
async fn queue_transfer_transfers_tokens_from_user_to_queue() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
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
    let (queue_pda, _queue_bump) = TransferQueue::find_pda(&mint);
    let queue_permission_pda = permission_pda_from_permissioned_account(&queue_pda);
    let queue_ata_pda = utils::derive_associated_token_address(&queue_pda, &mint);
    let (queue_eata_pda, queue_eata_bump) =
        Pubkey::find_program_address(&[queue_pda.as_ref(), mint.as_ref()], &PROGRAM);
    let queue_eata_permission_pda = permission_pda_from_permissioned_account(&queue_eata_pda);
    let shuttle_id = 0;
    let (shuttle_pda, _shuttle_bump) = ShuttleEphemeralAta::find_pda(&queue_pda, &mint, shuttle_id);
    let shuttle_ata_pda = utils::derive_associated_token_address(&shuttle_pda, &mint);
    let (shuttle_eata_pda, _shuttle_eata_bump) =
        Pubkey::find_program_address(&[shuttle_pda.as_ref(), mint.as_ref()], &PROGRAM);

    // Assert initial SPL token balances
    let user_token_acc_before = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state_before = Account::unpack(&user_token_acc_before.data).unwrap();
    assert_eq!(user_token_state_before.amount, STARTING_BALANCE);

    // Initialize Transfer Queue
    let ix_initialize_transfer_queue = Instruction::new_with_bytes(
        PROGRAM,
        &vec![
            vec![instruction::INITIALIZE_TRANSFER_QUEUE, queue_eata_bump],
            shuttle_id.to_le_bytes().to_vec(),
        ]
        .concat(),
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue_pda, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(queue_permission_pda, false),
            AccountMeta::new(queue_ata_pda, false),
            AccountMeta::new(queue_eata_pda, false),
            AccountMeta::new(queue_eata_permission_pda, false),
            AccountMeta::new(shuttle_pda, false),
            AccountMeta::new(shuttle_ata_pda, false),
            AccountMeta::new(shuttle_eata_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(associated_token_program_id(), false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
    );

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_initialize_transfer_queue],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

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
        .get_account(queue_ata_pda)
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
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(user, true), // [signer] user source token acc
            AccountMeta::new(user_ata, false),     // [writable] user source token acc
            AccountMeta::new(queue_pda, false),    // [writable] queue token acc
            AccountMeta::new(queue_ata_pda, false), // [writable] queue token acc
            AccountMeta::new_readonly(mint, false), // [] Mint pubkey (seed/consistency)
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
        .get_account(queue_ata_pda)
        .await
        .unwrap()
        .expect("queue token account must exist after deposit");
    let queue_token_state_after = Account::unpack(&queue_token_acc_after.data).unwrap();
    assert_eq!(queue_token_state_after.amount, amount);

    // Read back the Ephemeral ATA and verify amount incremented
    let account = context
        .banks_client
        .get_account(queue_pda)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(account.owner, PROGRAM);
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
        destination: queue_pda,
    };
    assert_eq!(queue_data.queue[0], expected_transfer);
}
