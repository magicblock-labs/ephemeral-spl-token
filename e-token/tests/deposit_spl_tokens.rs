use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
async fn deposit_spl_tokens_increments_ephemeral_amount() {
    let mut context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    let payer = context.payer.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive PDAs and setup mint/accounts via utils
    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        pdas.vault,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let ephemeral_ata = pdas.ephemeral_ata;
    let bump_ata = pdas.bump_ata;
    let vault = pdas.vault;
    let bump_vault = pdas.bump_vault;
    let user_ata = setup.user_tokens[0];
    let vault_ata = setup.vault_token;

    // Assert initial SPL token balances
    let user_token_acc_before = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state_before = Account::unpack(&user_token_acc_before.data).unwrap();
    assert_eq!(user_token_state_before.amount, STARTING_BALANCE);

    let vault_token_acc_before = context
        .banks_client
        .get_account(vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist");
    let vault_token_state_before = Account::unpack(&vault_token_acc_before.data).unwrap();
    assert_eq!(vault_token_state_before.amount, 0);

    // 1) Initialize Ephemeral ATA
    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump_ata],
    };

    // 2) Initialize Global Vault
    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, bump_vault],
    };

    // Send both initializations in one tx
    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    // 3) Deposit amount from payer's token to vault's token and increment Ephemeral ATA amount
    let amount: u64 = 100 * 10u64.pow(DECIMALS as u32);
    let mut data = vec![instruction::DEPOSIT_SPL_TOKENS];
    data.extend_from_slice(&amount.to_le_bytes());

    let ix_deposit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false), // [writable] Ephemeral ATA data
            AccountMeta::new_readonly(vault, false), // [] Global vault data
            AccountMeta::new_readonly(mint, false), // [] Mint pubkey (seed/consistency)
            AccountMeta::new(user_ata, false),      // [writable] user source token acc
            AccountMeta::new(vault_ata, false),     // [writable] vault token acc
            AccountMeta::new_readonly(payer, true), // [signer] user authority
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program id (readonly)
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_deposit],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Assert SPL token balances after deposit
    let user_token_acc_after = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist after deposit");
    let user_token_state_after = Account::unpack(&user_token_acc_after.data).unwrap();
    assert_eq!(user_token_state_after.amount, STARTING_BALANCE - amount);

    let vault_token_acc_after = context
        .banks_client
        .get_account(vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist after deposit");
    let vault_token_state_after = Account::unpack(&vault_token_acc_after.data).unwrap();
    assert_eq!(vault_token_state_after.amount, amount);

    // Read back the Ephemeral ATA and verify amount incremented
    let account = context
        .banks_client
        .get_account(ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ata account must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), EphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let ata_data = unsafe { load_mut_unchecked::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(ata_data.amount, amount);
}
