mod setup;

use {
    ephemeral_spl_api::{instruction, EphemeralAta, ID},
    solana_instruction::{AccountMeta, Instruction},
    solana_keypair::Keypair,
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_system_interface::instruction::create_account,
    solana_transaction::Transaction,
};

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn initialize_ephemeral_ata() {
    let mut context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    // Derive two arbitrary seeds (no PDA verification is performed by the program ATM)
    let payer_seed = Pubkey::new_unique();
    let mint_seed = Pubkey::new_unique();

    // Create the ephemeral ATA account owned by our program with proper space
    let ephemeral_ata = Keypair::new();

    let rent = context.banks_client.get_rent().await.unwrap();
    let space = EphemeralAta::LEN as u64;
    let lamports = rent.minimum_balance(space as usize);

    let create_ix = create_account(
        &context.payer.pubkey(),
        &ephemeral_ata.pubkey(),
        lamports,
        space,
        &PROGRAM,
    );

    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&context.payer.pubkey()),
        &[&context.payer, &ephemeral_ata],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Build our program instruction: discriminator 1 = InitializeEphemeralAta
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata.pubkey(), false), // writable account
            AccountMeta::new_readonly(payer_seed, false),     // payer seed (readonly)
            AccountMeta::new_readonly(mint_seed, false),      // mint seed  (readonly)
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Read back the account and ensure it was zero-initialized
    let account = context
        .banks_client
        .get_account(ephemeral_ata.pubkey())
        .await
        .unwrap()
        .expect("ephemeral ata account must exist");

    assert_eq!(account.owner, PROGRAM); // owned by the program
    assert_eq!(account.data.len(), EphemeralAta::LEN);

    let mut first_eight = [0u8; 8];
    first_eight.copy_from_slice(&account.data[..8]);
    let balance = u64::from_le_bytes(first_eight);
    assert_eq!(balance, 0);
}
