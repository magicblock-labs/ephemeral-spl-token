use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::RawType;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::bpf_loader;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn delegate_ephemeral_ata_succeeds() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    // Setup the delegation program
    let data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive the PDAs for our program and setup token accounts
    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        pdas.vault,
        6,
        1_000,
        1,
    )
    .await;

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
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
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
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),      // payer (signer)
            AccountMeta::new(pdas.ephemeral_ata, false), // ephemeral_ata (PDA)
            AccountMeta::new_readonly(PROGRAM, false),   // owner_program (this program)
            AccountMeta::new(buffer_pda, false),         // buffer PDA (created in CPI)
            AccountMeta::new(delegation_record_pda, false), // delegation record PDA
            AccountMeta::new(delegation_metadata_pda, false), // delegation metadata PDA
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID.into(), false), // delegation program
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
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into()
    );

    let _ = setup;
}

#[tokio::test]
async fn delegate_ephemeral_ata_non_owner_succeeds() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = Pubkey::new_unique();

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        pdas.vault,
        6,
        1_000,
        1,
    )
    .await;

    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, pdas.bump_ata],
    };

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
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
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", pdas.ephemeral_ata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID.into(), false),
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
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into()
    );

    let _ = setup;
}
