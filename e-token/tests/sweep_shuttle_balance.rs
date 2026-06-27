use ephemeral_rollups_pinocchio::spl::EphemeralAta;
use ephemeral_spl_api::{
    instruction,
    state::{load_initialized, shuttle_ephemeral_ata::ShuttleMetadata, RawType},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_pack::Pack;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account;

mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn sweep_shuttle_balance_transfers_from_shuttle_ata_to_destination() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let owner = payer;
    let shuttle_id = 42_u32;

    let mint_kp = utils::test_keypair("sweep_shuttle_balance::mint");
    let mint = mint_kp.pubkey();

    let (shuttle_ephemeral_ata, _) = ShuttleMetadata::find_pda(&owner, &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle_ephemeral_ata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_ephemeral_ata, mint);

    let setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, STARTING_BALANCE, 1).await;
    let destination_ata = setup.user_tokens[0];

    let mut shuttle_init_data = instruction::ESplInstruction::InitializeShuttle.to_vec();
    shuttle_init_data.extend_from_slice(&shuttle_id.to_le_bytes());
    let ix_init_shuttle = Instruction {
        program_id: PROGRAM,
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

    let tx_init =
        Transaction::new_signed_with_payer(&[ix_init_shuttle], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let amount: u64 = 700 * 10u64.pow(DECIMALS as u32);
    let mut ix_fund_shuttle = spl_token_interface::instruction::mint_to(
        &spl_token_interface::ID,
        &mint,
        &shuttle_wallet_ata,
        &payer,
        &[],
        amount,
    )
    .unwrap();
    ix_fund_shuttle.program_id = spl_token_interface::ID;
    let tx_fund_shuttle =
        Transaction::new_signed_with_payer(&[ix_fund_shuttle], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx_fund_shuttle).await.unwrap();

    let destination_before_merge = context
        .banks_client
        .get_account(destination_ata)
        .await
        .unwrap()
        .expect("destination account must exist");
    let destination_before_merge_state = Account::unpack(&destination_before_merge.data).unwrap();

    let shuttle_wallet_before_merge = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_before_merge_state = Account::unpack(&shuttle_wallet_before_merge.data).unwrap();
    assert_eq!(shuttle_wallet_before_merge_state.amount, amount);

    let ix_merge = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: instruction::ESplInstruction::SweepShuttleBalance.to_vec(),
    };

    let tx_merge = Transaction::new_signed_with_payer(&[ix_merge], Some(&payer), &[&payer_kp], context.last_blockhash);
    common::metrics::process_transaction_record_cu(&context.banks_client, tx_merge, "merge_shuttle::merge")
        .await
        .unwrap();

    let destination_after_merge = context
        .banks_client
        .get_account(destination_ata)
        .await
        .unwrap()
        .expect("destination account must exist");
    let destination_after_merge_state = Account::unpack(&destination_after_merge.data).unwrap();
    assert_eq!(
        destination_after_merge_state.amount,
        destination_before_merge_state.amount + amount
    );

    let shuttle_wallet_after_merge = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_after_merge_state = Account::unpack(&shuttle_wallet_after_merge.data).unwrap();
    assert_eq!(shuttle_wallet_after_merge_state.amount, 0);

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must still exist");
    assert_eq!(shuttle_account.owner, PROGRAM);
    assert_eq!(shuttle_account.data.len(), ShuttleMetadata::LEN);
    let mut shuttle_data = shuttle_account.data.clone();
    let shuttle = load_initialized::<ShuttleMetadata>(shuttle_data.as_mut_slice()).unwrap();
    assert_eq!(shuttle.id, shuttle_id);
    assert_eq!(shuttle.owner.as_array(), &owner.to_bytes());
    assert_eq!(shuttle.payer.as_array(), &payer.to_bytes());
}
