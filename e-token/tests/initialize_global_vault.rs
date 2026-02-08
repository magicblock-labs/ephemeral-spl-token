use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use {
    ephemeral_spl_api::instruction,
    solana_instruction::AccountMeta,
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};
mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6; // canonical USDC decimals

#[tokio::test]
async fn initialize_global_vault() {
    let mut context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    let payer = context.payer.pubkey();
    let user = payer;

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        1,
    )
    .await;

    let vault_token_acc = spl_associated_token_account::get_associated_token_address(&pdas.vault, &mint);

    // Build instruction
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),          // writable vault account
            AccountMeta::new_readonly(payer, false), // payer (funds, not part of seeds)
            AccountMeta::new_readonly(mint, false),  // mint (seed)
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program
            AccountMeta::new_readonly(spl_associated_token_account::ID, false), // associated token program
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, pdas.bump_vault],
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
        .get_account(pdas.vault)
        .await
        .unwrap()
        .expect("global vault must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), GlobalVault::LEN);

    let mut mut_acc = account.data.clone();
    let vault_data = unsafe { load_mut_unchecked::<GlobalVault>(mut_acc.as_mut_slice()).unwrap() };
    assert!(vault_data.is_initialized());
}
