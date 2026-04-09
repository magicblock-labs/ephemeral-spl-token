use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load, RawType};
use ephemeral_spl_api::ID as PROGRAM;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {solana_program_test::tokio, solana_signer::Signer, solana_transaction::Transaction};

mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn withdraw_spl_tokens_decrements_ephemeral_amount() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = utils::test_keypair("withdraw_spl_tokens::mint");
    let mint = mint_kp.pubkey();

    // Derive PDAs and setup mint/accounts via utils
    let pdas = utils::derive_pdas(PROGRAM, user, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        2,
    )
    .await;

    let ephemeral_ata = pdas.ephemeral_ata;
    let vault = pdas.vault;
    let user_source = setup.user_tokens[0];
    let user_dest = setup.user_tokens[1];
    let (vault_eata, _) = EphemeralAta::find_pda(&vault, &mint);
    let vault_token = utils::derive_associated_token_address(vault, mint);

    // Initialize Ephemeral ATA
    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA],
    };

    // Initialize Global Vault
    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_token, false), // vault token account
            AccountMeta::new_readonly(spl_token_interface::ID, false), // token program
            AccountMeta::new_readonly(utils::associated_token_program_id(), false), // associated token program
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    // Deposit first to fund the vault and set ephemeral amount
    let deposit_amount: u64 = 1_000 * 10u64.pow(DECIMALS as u32);
    let mut deposit_data = vec![instruction::DEPOSIT_SPL_TOKENS];
    deposit_data.extend_from_slice(&deposit_amount.to_le_bytes());
    let ix_deposit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(user_source, false),
            AccountMeta::new(vault_token, false),
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: deposit_data,
    };
    let tx_deposit = Transaction::new_signed_with_payer(
        &[ix_deposit],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_deposit)
        .await
        .unwrap();

    // Now withdraw a portion
    let withdraw_amount: u64 = 400 * 10u64.pow(DECIMALS as u32);
    let mut withdraw_data = vec![instruction::WITHDRAW_SPL_TOKENS];
    withdraw_data.extend_from_slice(&withdraw_amount.to_le_bytes());

    let ix_withdraw = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),  // [signer]
            AccountMeta::new(ephemeral_ata, false),  // [writable]
            AccountMeta::new_readonly(vault, false), // [] vault data
            AccountMeta::new_readonly(mint, false),  // [] mint
            AccountMeta::new(vault_token, false),    // [writable] source (vault)
            AccountMeta::new(user_dest, false),      // [writable] destination (user)
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program
        ],
        data: withdraw_data,
    };

    let tx_withdraw = Transaction::new_signed_with_payer(
        &[ix_withdraw],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_withdraw,
        "wd_spl::withdraw",
    )
    .await
    .unwrap();

    // Check SPL balances
    let vault_after = context
        .banks_client
        .get_account(vault_token)
        .await
        .unwrap()
        .expect("vault token exists");
    let vault_state_after = Account::unpack(&vault_after.data).unwrap();
    assert_eq!(vault_state_after.amount, deposit_amount - withdraw_amount);

    let user_dest_after = context
        .banks_client
        .get_account(user_dest)
        .await
        .unwrap()
        .expect("user dest token exists");
    let user_dest_state_after = Account::unpack(&user_dest_after.data).unwrap();
    assert_eq!(user_dest_state_after.amount, withdraw_amount);

    // Check Ephemeral ATA decreased accordingly
    let account = context
        .banks_client
        .get_account(ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ata account must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), EphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let ata_data = load::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap();
    assert_eq!(ata_data.amount, deposit_amount - withdraw_amount);
}
