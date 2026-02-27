use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program,
    delegation_metadata_pda_from_delegated_account, delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::instruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::{initialize_transfer_queue, setup_program_test};

#[tokio::test]
async fn test_delegate_transfer_queue() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = Pubkey::new_unique();
    let validator = payer;

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let queue_shuttle_id = 0;
    let pdas = utils::derive_pdas(ephemeral_spl_api::program::ID, user, mint, queue_shuttle_id);
    utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;

    initialize_transfer_queue(&mut context, payer, mint, queue_shuttle_id, &pdas).await;

    let queue_buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(
        &pdas.queue,
        &ephemeral_spl_api::program::ID,
    );
    let queue_delegation_record_pda = delegation_record_pda_from_delegated_account(&pdas.queue);
    let queue_delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&pdas.queue);

    let eata_buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(
        &pdas.queue_eata,
        &ephemeral_spl_api::program::ID,
    );
    let eata_delegation_record_pda = delegation_record_pda_from_delegated_account(&pdas.queue_eata);
    let eata_delegation_metadata_pda =
        delegation_metadata_pda_from_delegated_account(&pdas.queue_eata);

    let ix_delegate = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(pdas.queue, false),
            AccountMeta::new(pdas.queue_eata, false),
            AccountMeta::new_readonly(ephemeral_spl_api::program::ID, false),
            AccountMeta::new(queue_buffer_pda, false),
            AccountMeta::new(queue_delegation_record_pda, false),
            AccountMeta::new(queue_delegation_metadata_pda, false),
            AccountMeta::new(eata_buffer_pda, false),
            AccountMeta::new(eata_delegation_record_pda, false),
            AccountMeta::new(eata_delegation_metadata_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::DELEGATE_TRANSFER_QUEUE, pdas.queue_eata_bump],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );

    eprintln!("pdas: {:#?}", pdas);
    eprintln!("queue_buffer_pda: {:#?}", queue_buffer_pda);
    eprintln!(
        "queue_delegation_record_pda: {:#?}",
        queue_delegation_record_pda
    );
    eprintln!(
        "queue_delegation_metadata_pda: {:#?}",
        queue_delegation_metadata_pda
    );
    eprintln!("eata_buffer_pda: {:#?}", eata_buffer_pda);
    eprintln!(
        "eata_delegation_record_pda: {:#?}",
        eata_delegation_record_pda
    );
    eprintln!(
        "eata_delegation_metadata_pda: {:#?}",
        eata_delegation_metadata_pda
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let queue_account = context.banks_client.get_account(pdas.queue).await.unwrap();
    assert!(queue_account.is_some());
    assert_eq!(
        queue_account.unwrap().owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let eata_account = context
        .banks_client
        .get_account(pdas.queue_eata)
        .await
        .unwrap();
    assert!(eata_account.is_some());
    assert_eq!(
        eata_account.unwrap().owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
}
