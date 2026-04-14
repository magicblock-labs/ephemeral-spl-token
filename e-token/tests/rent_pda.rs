use ephemeral_spl_api::instruction;
use ephemeral_spl_api::ID as PROGRAM;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

const RENT_PDA_SEED: &[u8] = b"rent";

#[tokio::test]
async fn initialize_rent_pda_is_idempotent() {
    let context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_RENT_PDA],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "rent_pda::init")
        .await
        .unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_reinit =
        Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], second_blockhash);
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_reinit,
        "rent_pda::reinit",
    )
    .await
    .unwrap();

    let rent_pda_account = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must exist");

    assert_eq!(rent_pda_account.owner, solana_system_interface::program::ID);
    assert_eq!(rent_pda_account.data.len(), 0);
    assert!(rent_pda_account.lamports >= Rent::default().minimum_balance(0));
}
