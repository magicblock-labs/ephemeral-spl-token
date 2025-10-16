use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use solana_instruction::Instruction;
use {
    ephemeral_spl_api::instruction,
    solana_instruction::AccountMeta,
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn initialize_global_vault() {
    let context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    let payer = context.payer.pubkey();
    let mint = Pubkey::new_unique();

    // PDA derived only from [mint]
    let (vault, bump) = Pubkey::find_program_address(&[mint.to_bytes().as_slice()], &PROGRAM);

    // Build instruction
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),          // writable vault account
            AccountMeta::new_readonly(payer, false), // payer (funds, not part of seeds)
            AccountMeta::new_readonly(mint, false),  // mint (seed)
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, bump],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Verify account
    let account = context
        .banks_client
        .get_account(vault)
        .await
        .unwrap()
        .expect("global vault must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), GlobalVault::LEN);

    let mut mut_acc = account.data.clone();
    let vault_data = unsafe { load_mut_unchecked::<GlobalVault>(mut_acc.as_mut_slice()).unwrap() };
    assert!(vault_data.is_initialized());
}
