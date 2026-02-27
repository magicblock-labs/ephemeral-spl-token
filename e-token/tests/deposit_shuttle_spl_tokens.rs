use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {solana_program_test::tokio, solana_signer::Signer, solana_transaction::Transaction};

mod utils;

use crate::utils::setup_program_test;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn deposit_spl_tokens_increments_shuttle_amount() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let owner = payer;
    let shuttle_id = 11_u32;

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(ephemeral_spl_api::program::ID, owner, mint, 0);
    let (shuttle_ephemeral_ata, shuttle_bump) = utils::derive_shuttle_ephemeral_ata(
        ephemeral_spl_api::program::ID,
        owner,
        mint,
        shuttle_id,
    );
    let (shuttle_eata, _shuttle_eata_bump) =
        utils::derive_shuttle_eata(ephemeral_spl_api::program::ID, shuttle_ephemeral_ata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(&shuttle_ephemeral_ata, &mint);

    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let vault = pdas.vault;
    let vault_bump = pdas.bump_vault;
    let user_ata = setup.user_tokens[0];
    let vault_ata = utils::derive_associated_token_address(&vault, &mint);

    let mut shuttle_init_data = vec![instruction::INITIALIZE_SHUTTLE_EPHEMERAL_ATA];
    shuttle_init_data.extend_from_slice(&shuttle_id.to_le_bytes());
    shuttle_init_data.push(shuttle_bump);
    let ix_init_shuttle = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: shuttle_init_data,
    };

    let ix_init_vault = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, vault_bump],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_shuttle, ix_init_vault],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let amount: u64 = 100 * 10u64.pow(DECIMALS as u32);
    let mut data = vec![instruction::DEPOSIT_SPL_TOKENS];
    data.extend_from_slice(&amount.to_le_bytes());

    let ix_deposit = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(user_ata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
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

    let account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");

    assert_eq!(account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(account.data.len(), ShuttleEphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleEphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(shuttle.id, shuttle_id);
    assert_eq!(shuttle.owner.as_array(), &owner.to_bytes());
    assert_eq!(shuttle.payer.as_array(), &payer.to_bytes());

    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata must exist");
    assert_eq!(shuttle_eata_account.data.len(), EphemeralAta::LEN);
    let mut mut_shuttle_eata = shuttle_eata_account.data.clone();
    let shuttle_eata_data =
        unsafe { load_mut_unchecked::<EphemeralAta>(mut_shuttle_eata.as_mut_slice()).unwrap() };
    assert_eq!(shuttle_eata_data.amount, amount);
    assert_eq!(
        shuttle_eata_data.owner.as_array(),
        &shuttle_ephemeral_ata.to_bytes()
    );
}
