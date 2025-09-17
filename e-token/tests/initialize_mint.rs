mod setup;

use {
    solana_keypair::Keypair,
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};
use ephemeral_spl_api::ID;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn initialize_mint() {
    let mut context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    // Given a mint authority, freeze authority and an account keypair.
    let mint_authority = Pubkey::new_unique();
    let freeze_authority = Pubkey::new_unique();
    let account = Keypair::new();

    // Build initialize instruction and ensure it targets our test program id
    let mut initialize_ix = spl_token::instruction::initialize_mint(
        &spl_token::ID,
        &account.pubkey(),
        &mint_authority,
        Some(&freeze_authority),
        0,
    ).unwrap();
    initialize_ix.program_id = PROGRAM;

    // First create the mint account, then initialize it.
    let instructions = vec![
        initialize_ix,
    ];

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
}
