use ephemeral_spl_api::{
    instruction,
    state::{ephemeral_ata::EphemeralAta, global_vault::GlobalVault, load_initialized, RawType},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;
mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1;

#[tokio::test]
async fn initialize_global_vault() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer;
    let mint_kp = utils::test_keypair("initialize_global_vault::mint");
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let vault_token_acc = utils::derive_associated_token_address(pdas.vault, mint);
    let (vault_eata, _) = EphemeralAta::find_pda(&pdas.vault, &mint);

    // Build instruction
    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),    // writable vault account
            AccountMeta::new(payer, true),          // payer (funds, signer)
            AccountMeta::new_readonly(mint, false), // mint (seed)
            AccountMeta::new(vault_eata, false),    // vault ephemeral ATA
            AccountMeta::new(vault_token_acc, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // system program
        ],
        data: instruction::ESplInstruction::InitializeGlobalVault.to_vec(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "init_gvault::init")
        .await
        .unwrap();

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
    let vault_data = load_initialized::<GlobalVault>(mut_acc.as_mut_slice()).unwrap();
    assert_eq!(vault_data.mint, mint);

    let vault_token_acc_state = context
        .banks_client
        .get_account(vault_token_acc)
        .await
        .unwrap()
        .expect("vault token account must exist");
    assert_eq!(vault_token_acc_state.owner, spl_token_interface::ID);

    let vault_eata_account = context
        .banks_client
        .get_account(vault_eata)
        .await
        .unwrap()
        .expect("vault ephemeral ATA must exist");
    assert_eq!(vault_eata_account.owner, PROGRAM);
    assert_eq!(vault_eata_account.data.len(), EphemeralAta::LEN);
}
