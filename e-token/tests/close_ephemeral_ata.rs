use ephemeral_spl_api::instruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use {
    solana_program_test::tokio, solana_pubkey::Pubkey, solana_signer::Signer,
    solana_system_interface::instruction::create_account, solana_transaction::Transaction,
};

mod utils;

use crate::utils::setup_program_test;

#[tokio::test]
async fn close_ephemeral_ata_refunds_rent_and_closes_account() {
    let pt = setup_program_test();
    let context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer;
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) = Pubkey::find_program_address(
        &[user.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );

    let recipient_kp = Keypair::new();
    let recipient = recipient_kp.pubkey();

    let rent = context.banks_client.get_rent().await.unwrap();
    let ix_create_recipient = create_account(
        &payer,
        &recipient,
        rent.minimum_balance(0).max(1),
        0,
        &solana_system_interface::program::ID,
    );

    let ix_init = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_create_recipient, ix_init],
        Some(&payer),
        &[&context.payer, &recipient_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let recipient_before = context
        .banks_client
        .get_account(recipient)
        .await
        .unwrap()
        .expect("recipient account should exist");
    let recipient_before_lamports = recipient_before.lamports;

    let ephemeral_ata_before = context
        .banks_client
        .get_account(ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ATA should exist");
    let ephemeral_ata_lamports = ephemeral_ata_before.lamports;
    assert!(ephemeral_ata_lamports > 0);

    let ix_close = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(recipient, false),
        ],
        data: vec![instruction::CLOSE_EPHEMERAL_ATA],
    };

    let tx_close = Transaction::new_signed_with_payer(
        &[ix_close],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_close)
        .await
        .unwrap();

    let recipient_after = context
        .banks_client
        .get_account(recipient)
        .await
        .unwrap()
        .expect("recipient account should still exist");
    assert_eq!(
        recipient_after.lamports,
        recipient_before_lamports + ephemeral_ata_lamports
    );

    let ephemeral_ata_after = context
        .banks_client
        .get_account(ephemeral_ata)
        .await
        .unwrap();
    assert!(ephemeral_ata_after.is_none());
}
