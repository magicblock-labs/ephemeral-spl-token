use ephemeral_spl_api::{
    instruction,
    state::{ephemeral_ata::EphemeralAta, shuttle_ephemeral_ata::ShuttleMetadata},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_pack::Pack;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account as SplAccount;

mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);
const RECOVERY_AMOUNT: u64 = 100 * 10u64.pow(DECIMALS as u32);
const SHUTTLE_WALLET_AMOUNT: u64 = 7 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn recover_and_close_shuttle_to_owner_settles_full_balance_to_owner_token_account() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let owner = payer;
    let shuttle_id = 73_u32;

    let mint_kp = utils::test_keypair("recover_and_close_shuttle_to_owner::mint");
    let mint = mint_kp.pubkey();

    let setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, STARTING_BALANCE, 2).await;
    let owner_source_ata = setup.user_tokens[0];
    let owner_destination_ata = setup.user_tokens[1];

    let pdas = utils::derive_pdas(PROGRAM, owner, mint);
    let vault = pdas.vault;
    let (vault_eata, _) = EphemeralAta::find_pda(&vault, &mint);
    let vault_ata = utils::derive_associated_token_address(vault, mint);

    let (shuttle, _) = ShuttleMetadata::find_pda(&owner, &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle, mint);

    let ix_init_shuttle = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeShuttleEphemeralAta.with_data(&shuttle_id.to_le_bytes()),
    };

    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeGlobalVault.to_vec(),
    };

    let attacker_token = utils::test_keypair("recover_and_close_shuttle_to_owner::attacker_token");
    let attacker_owner = utils::test_pubkey("recover_and_close_shuttle_to_owner::attacker_owner");
    let rent = context.banks_client.get_rent().await.unwrap();
    let ix_create_attacker_token = solana_system_interface::instruction::create_account(
        &payer,
        &attacker_token.pubkey(),
        rent.minimum_balance(SplAccount::LEN),
        SplAccount::LEN as u64,
        &spl_token_interface::ID,
    );
    let mut ix_init_attacker_token = spl_token_interface::instruction::initialize_account(
        &spl_token_interface::ID,
        &attacker_token.pubkey(),
        &mint,
        &attacker_owner,
    )
    .unwrap();
    ix_init_attacker_token.program_id = spl_token_interface::ID;

    let tx_init = Transaction::new_signed_with_payer(
        &[
            ix_init_shuttle,
            ix_init_vault,
            ix_create_attacker_token,
            ix_init_attacker_token,
        ],
        Some(&payer),
        &[&payer_kp, &attacker_token],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let ix_deposit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(owner_source_ata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: instruction::ESplInstruction::DepositSplTokens.with_data(&RECOVERY_AMOUNT.to_le_bytes()),
    };
    let tx_deposit = Transaction::new_signed_with_payer(
        &[ix_deposit],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context.banks_client.process_transaction(tx_deposit).await.unwrap();

    let mut ix_fund_shuttle_wallet = spl_token_interface::instruction::transfer_checked(
        &spl_token_interface::ID,
        &owner_source_ata,
        &mint,
        &shuttle_wallet_ata,
        &owner,
        &[],
        SHUTTLE_WALLET_AMOUNT,
        DECIMALS,
    )
    .unwrap();
    ix_fund_shuttle_wallet.program_id = spl_token_interface::ID;
    let tx_fund_shuttle_wallet = Transaction::new_signed_with_payer(
        &[ix_fund_shuttle_wallet],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(tx_fund_shuttle_wallet)
        .await
        .unwrap();

    let bad_recovery = recover_ix(
        payer,
        shuttle,
        shuttle_eata,
        shuttle_wallet_ata,
        attacker_token.pubkey(),
        mint,
        vault,
        vault_ata,
    );
    let tx_bad_recovery = Transaction::new_signed_with_payer(
        &[bad_recovery],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    assert!(context.banks_client.process_transaction(tx_bad_recovery).await.is_err());

    let bad_wallet_recovery = recover_ix(
        payer,
        shuttle,
        shuttle_eata,
        attacker_token.pubkey(),
        owner_destination_ata,
        mint,
        vault,
        vault_ata,
    );
    let tx_bad_wallet_recovery = Transaction::new_signed_with_payer(
        &[bad_wallet_recovery],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    assert!(context
        .banks_client
        .process_transaction(tx_bad_wallet_recovery)
        .await
        .is_err());

    let ix_recover = recover_ix(
        payer,
        shuttle,
        shuttle_eata,
        shuttle_wallet_ata,
        owner_destination_ata,
        mint,
        vault,
        vault_ata,
    );
    let tx_recover = Transaction::new_signed_with_payer(
        &[ix_recover],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx_recover, "recover_shuttle::close")
        .await
        .unwrap();

    let owner_destination_after = context
        .banks_client
        .get_account(owner_destination_ata)
        .await
        .unwrap()
        .expect("owner destination token account must exist");
    let owner_destination_state = SplAccount::unpack(&owner_destination_after.data).unwrap();
    assert_eq!(owner_destination_state.amount, RECOVERY_AMOUNT + SHUTTLE_WALLET_AMOUNT);

    let vault_after = context
        .banks_client
        .get_account(vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist");
    let vault_state = SplAccount::unpack(&vault_after.data).unwrap();
    assert_eq!(vault_state.amount, 0);

    assert!(context.banks_client.get_account(shuttle).await.unwrap().is_none());
    assert!(context.banks_client.get_account(shuttle_eata).await.unwrap().is_none());
    assert!(context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .is_none());
}

#[allow(clippy::too_many_arguments)]
fn recover_ix(
    rent_reimbursement: solana_pubkey::Pubkey,
    shuttle: solana_pubkey::Pubkey,
    shuttle_eata: solana_pubkey::Pubkey,
    shuttle_wallet_ata: solana_pubkey::Pubkey,
    destination_token: solana_pubkey::Pubkey,
    mint: solana_pubkey::Pubkey,
    vault: solana_pubkey::Pubkey,
    vault_token: solana_pubkey::Pubkey,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(rent_reimbursement, false),
            AccountMeta::new(shuttle, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new(destination_token, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(vault_token, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: instruction::ESplInstruction::RecoverAndCloseShuttleToOwner.to_vec(),
    }
}
