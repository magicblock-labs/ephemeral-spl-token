use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::{tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
const RENT_PDA_SEED: &[u8] = b"rent";

#[tokio::test]
async fn initialize_rent_pda_is_idempotent() {
    let pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    let context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
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
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_reinit = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&context.payer],
        second_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_reinit)
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
