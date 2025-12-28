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

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn withdraw_spl_tokens_decrements_ephemeral_amount() {
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
        2,
    )
    .await;

    let ephemeral_ata = pdas.ephemeral_ata;
    let bump_ata = pdas.bump_ata;
    let vault = pdas.vault;
    let bump_vault = pdas.bump_vault;
    let user_source = setup.user_tokens[0];
    let user_dest = setup.user_tokens[1];
    let vault_token = setup.vault_token;

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
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump_ata],
    };

    // Initialize Global Vault
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
        &[&context.payer],
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
    withdraw_data.push(bump_vault); // provide vault bump for PDA signing

    let ix_withdraw = Instruction {
        program_id: PROGRAM,
        accounts: vec![
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
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_withdraw)
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
    let ata_data = unsafe { load_mut_unchecked::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(ata_data.amount, deposit_amount - withdraw_amount);
}
