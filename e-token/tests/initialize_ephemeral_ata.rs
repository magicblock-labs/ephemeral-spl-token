use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use ephemeral_spl_api::ID as PROGRAM;
use solana_instruction::Instruction;
use {
    ephemeral_spl_api::instruction, solana_instruction::AccountMeta, solana_program_test::tokio,
    solana_pubkey::Pubkey, solana_signer::Signer, solana_transaction::Transaction,
};

mod common;
mod utils;

#[tokio::test]
async fn initialize_ephemeral_ata() {
    let context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = utils::test_pubkey("initialize_ephemeral_ata::user");
    let mint = utils::test_pubkey("initialize_ephemeral_ata::mint");

    // Create the ephemeral ATA account owned by our program with proper space
    let (ephemeral_ata, _) = Pubkey::find_program_address(
        &[user.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &PROGRAM,
    );

    // Build our program instruction: discriminator 1 = InitializeEphemeralAta
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),  // writable account
            AccountMeta::new_readonly(payer, false), // payer (funding)
            AccountMeta::new_readonly(user, false),  // user seed (readonly)
            AccountMeta::new_readonly(mint, false),  // mint seed  (readonly)
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program (readonly)
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA], // instruction data: discriminator
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "init_eata::init",
    )
    .await
    .unwrap();

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
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert!(ephemeral_ata.is_initialized());
    assert_eq!(ephemeral_ata.amount, 0);
    assert_eq!(ephemeral_ata.owner.as_array(), &user.to_bytes());
}
