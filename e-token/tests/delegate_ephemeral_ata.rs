use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program,
    delegation_metadata_pda_from_delegated_account, delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::{
    error::EphemeralSplError,
    instruction,
    instructions::DelegateArgs,
    state::{ephemeral_ata::EphemeralAta, RawType},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program::instruction::InstructionError;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::{Transaction, TransactionError};
use wheels::layout::Encodable as _;

mod common;
mod utils;

#[tokio::test]
async fn delegate_ephemeral_ata_succeeds() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = utils::test_keypair("delegate_ephemeral_ata_succeeds::mint");
    let mint = mint_kp.pubkey();
    let validator = utils::test_pubkey("delegate_ephemeral_ata_succeeds::validator");
    let other_validator = utils::test_pubkey("delegate_ephemeral_ata_succeeds::other_validator");

    // Derive the PDAs for our program and setup token accounts
    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 1_000, 1).await;

    // Initialize the Ephemeral ATA and Global Vault (required by the program state)
    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeEphemeralAta.to_vec(),
    };

    let vault_token_acc = utils::derive_associated_token_address(pdas.vault, mint);
    let (vault_eata, _) = EphemeralAta::find_pda(&pdas.vault, &mint);

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeGlobalVault.to_vec(),
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&payer_kp],
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
    let buffer_pda =
        delegate_buffer_pda_from_delegated_account_and_owner_program(&pdas.ephemeral_ata, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&pdas.ephemeral_ata);
    let delegation_metadata_pda =
        delegation_metadata_pda_from_delegated_account(&pdas.ephemeral_ata);

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),      // payer (signer)
            AccountMeta::new(pdas.ephemeral_ata, false), // ephemeral_ata (PDA)
            AccountMeta::new_readonly(PROGRAM, false),   // owner_program (this program)
            AccountMeta::new(buffer_pda, false),         // buffer PDA (created in CPI)
            AccountMeta::new(delegation_record_pda, false), // delegation record PDA
            AccountMeta::new(delegation_metadata_pda, false), // delegation metadata PDA
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false), // delegation program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program
        ],
        data: instruction::ESplInstruction::DelegateEphemeralAta.with_data(
            &DelegateArgs {
                validator: Some(validator),
            }
            .encode()
            .unwrap(),
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );

    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "del_eata::delegate")
        .await
        .unwrap();

    let redelegate_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    let tx_redelegate = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&payer_kp],
        redelegate_blockhash,
    );

    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_redelegate,
        "del_eata::redelegate_same_validator",
    )
    .await
    .unwrap();

    let ix_redelegate_other_validator = Instruction {
        data: instruction::ESplInstruction::DelegateEphemeralAta.with_data(
            &DelegateArgs {
                validator: Some(other_validator),
            }
            .encode()
            .unwrap(),
        ),
        ..ix_delegate
    };

    let redelegate_other_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    let tx_redelegate_other = Transaction::new_signed_with_payer(
        &[ix_redelegate_other_validator],
        Some(&payer),
        &[&payer_kp],
        redelegate_other_blockhash,
    );

    let redelegate_other_result = common::metrics::process_transaction_with_metadata_recorded(
        &context.banks_client,
        tx_redelegate_other,
        "del_eata::redelegate_other_validator",
    )
    .await
    .unwrap();
    assert_eq!(
        redelegate_other_result.result.unwrap_err(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(EphemeralSplError::EphemeralAtaValidatorMismatch as u32),
        )
    );
    let logs = redelegate_other_result
        .metadata
        .expect("failed transaction should include metadata")
        .log_messages;
    assert!(logs.iter().any(|log| log.contains(&validator.to_string())));

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

    // Re-running InitializeEphemeralAta must be idempotent even when delegated.
    let ix_reinit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeEphemeralAta.to_vec(),
    };

    let reinit_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    let tx_reinit = Transaction::new_signed_with_payer(
        &[ix_reinit],
        Some(&payer),
        &[&payer_kp],
        reinit_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_reinit,
        "del_eata::reinit",
    )
    .await
    .unwrap();

    let ata_account_after_reinit = context
        .banks_client
        .get_account(pdas.ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ata should still exist");
    assert_eq!(
        ata_account_after_reinit.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let _ = setup;
}

#[tokio::test]
async fn delegate_ephemeral_ata_non_owner_succeeds() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = utils::test_pubkey("delegate_ephemeral_ata_non_owner_succeeds::user");

    let mint_kp = utils::test_keypair("delegate_ephemeral_ata_non_owner_succeeds::mint");
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 1_000, 1).await;

    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeEphemeralAta.to_vec(),
    };

    let vault_token_acc = utils::derive_associated_token_address(pdas.vault, mint);
    let (vault_eata, _) = EphemeralAta::find_pda(&pdas.vault, &mint);

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeGlobalVault.to_vec(),
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let buffer_pda =
        delegate_buffer_pda_from_delegated_account_and_owner_program(&pdas.ephemeral_ata, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&pdas.ephemeral_ata);
    let delegation_metadata_pda =
        delegation_metadata_pda_from_delegated_account(&pdas.ephemeral_ata);

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::DelegateEphemeralAta
            .with_data(&DelegateArgs { validator: None }.encode().unwrap()),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );

    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "del_eata::non_owner",
    )
    .await
    .unwrap();

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
