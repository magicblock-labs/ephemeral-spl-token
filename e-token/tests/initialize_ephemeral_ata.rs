
use solana_instruction::Instruction;
use {
    ephemeral_spl_api::{instruction},
    solana_instruction::{AccountMeta},
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn initialize_ephemeral_ata() {
    let context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    // Derive two arbitrary seeds
    let payer = context.payer.pubkey();
    let mint = Pubkey::new_unique();

    // Create the ephemeral ATA account owned by our program with proper space
    let (ephemeral_ata, bump) = Pubkey::find_program_address(&[payer.to_bytes().as_slice(), mint.to_bytes().as_slice()], &PROGRAM);

    // Build our program instruction: discriminator 1 = InitializeEphemeralAta
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),      // writable account
            AccountMeta::new_readonly(payer, false),     // payer seed (readonly)
            AccountMeta::new_readonly(mint, false),      // mint seed  (readonly)
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),      // system program (readonly)
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump], // instruction data: discriminator + bump
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
        .get_account(ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ata account must exist");

    assert_eq!(account.owner, PROGRAM); // owned by the program
    assert_eq!(account.data.len(), EphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let ephemeral_ata = unsafe { load_mut_unchecked::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap()};
    assert!(ephemeral_ata.is_initialized());
    assert_eq!(ephemeral_ata.amount, 0);
}
