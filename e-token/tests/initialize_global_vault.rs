use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use solana_account::Account as SolanaAccount;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program::rent::Rent;
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
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1;

#[tokio::test]
async fn initialize_global_vault() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    let mut context = pt.start_with_context().await;

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
        STARTING_BALANCE,
        1,
    )
    .await;

    let vault_token_acc = utils::derive_associated_token_address(pdas.vault, mint);
    let (vault_eata, _) =
        Pubkey::find_program_address(&[pdas.vault.as_ref(), mint.as_ref()], &PROGRAM);

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
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
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

#[tokio::test]
async fn initialize_global_vault_migrates_legacy_layout() {
    let user = Pubkey::new_unique();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();
    let pdas = utils::derive_pdas(PROGRAM, user, mint);

    let legacy_lamports = Rent::default().minimum_balance(32);
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    pt.add_account(
        pdas.vault,
        SolanaAccount {
            lamports: legacy_lamports,
            data: mint.to_bytes().to_vec(),
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();

    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let vault_token_acc = utils::derive_associated_token_address(pdas.vault, mint);
    let (vault_eata, _) =
        Pubkey::find_program_address(&[pdas.vault.as_ref(), mint.as_ref()], &PROGRAM);

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_token_acc, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let account = context
        .banks_client
        .get_account(pdas.vault)
        .await
        .unwrap()
        .expect("migrated vault must exist");
    assert_eq!(account.data.len(), GlobalVault::LEN);

    let rent = context.banks_client.get_rent().await.unwrap();
    assert!(account.lamports >= rent.minimum_balance(GlobalVault::LEN));

    let mut mut_acc = account.data.clone();
    let vault_data = unsafe { load_mut_unchecked::<GlobalVault>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(
        vault_data.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(
        vault_data.token_account,
        ephemeral_spl_api::Address::new_from_array(vault_token_acc.to_bytes())
    );

    let vault_eata_account = context
        .banks_client
        .get_account(vault_eata)
        .await
        .unwrap()
        .expect("vault ephemeral ATA must exist after migration");
    assert_eq!(vault_eata_account.owner, PROGRAM);
    assert_eq!(vault_eata_account.data.len(), EphemeralAta::LEN);
}
