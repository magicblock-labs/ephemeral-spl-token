use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::RawType;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::setup_program_test;

#[tokio::test]
async fn delegate_ephemeral_ata_succeeds() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive the PDAs for our program and setup token accounts
    let pdas = utils::derive_pdas(ephemeral_spl_api::program::ID, user, mint, 0);
    let setup =
        utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;

    // Initialize the Ephemeral ATA and Global Vault (required by the program state)
    let ix_init_ata = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let vault_token_acc = utils::derive_associated_token_address(&pdas.vault, &mint);

    let ix_init_vault = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, pdas.bump_vault],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    // Verify the Ephemeral ATA was initialized
    let ephemeral_ata_account = context
        .banks_client
        .get_account(pdas.ephemeral_ata)
        .await
        .unwrap();
    assert!(ephemeral_ata_account.is_some());
    assert_eq!(
        ephemeral_ata_account.unwrap().data.len(),
        ephemeral_spl_api::state::ephemeral_ata::EphemeralAta::LEN
    );

    // Derive required PDAs
    let (buffer_pda, _) = Pubkey::find_program_address(
        &[b"buffer", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::id().into(),
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_delegate = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),      // payer (signer)
            AccountMeta::new(pdas.ephemeral_ata, false), // ephemeral_ata (PDA)
            AccountMeta::new_readonly(ephemeral_spl_api::program::ID, false), // owner_program (this program)
            AccountMeta::new(buffer_pda, false), // buffer PDA (created in CPI)
            AccountMeta::new(delegation_record_pda, false), // delegation record PDA
            AccountMeta::new(delegation_metadata_pda, false), // delegation metadata PDA
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false), // delegation program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    // Assert ATA is owned by delegation program after delegation
    let ata_account = context
        .banks_client
        .get_account(pdas.ephemeral_ata)
        .await
        .unwrap();
    assert!(ata_account.is_some());
    assert_eq!(
        ata_account.unwrap().owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let _ = setup;
}

#[tokio::test]
async fn delegate_ephemeral_ata_non_owner_succeeds() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = Pubkey::new_unique();

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(ephemeral_spl_api::program::ID, user, mint, 0);
    let setup =
        utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;

    let ix_init_ata = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let vault_token_acc = utils::derive_associated_token_address(&pdas.vault, &mint);

    let ix_init_vault = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, pdas.bump_vault],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let (buffer_pda, _) = Pubkey::find_program_address(
        &[b"buffer", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::id().into(),
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_delegate = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(ephemeral_spl_api::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let ata_account = context
        .banks_client
        .get_account(pdas.ephemeral_ata)
        .await
        .unwrap();
    assert!(ata_account.is_some());
    assert_eq!(
        ata_account.unwrap().owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let _ = setup;
}
