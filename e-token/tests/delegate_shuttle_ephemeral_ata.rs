use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program, delegation_metadata_pda_from_delegated_account,
    delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::{
    instruction,
    instructions::DelegateShuttleArgs,
    state::{ephemeral_ata::EphemeralAta, shuttle_ephemeral_ata::ShuttleMetadata, RawType},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

#[tokio::test]
async fn delegate_shuttle_ephemeral_ata_succeeds() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let owner = utils::test_pubkey("delegate_shuttle_ephemeral_ata::owner");
    let mint_kp = utils::test_keypair("delegate_shuttle_ephemeral_ata::mint");
    let mint = mint_kp.pubkey();
    let shuttle_id = 9_u32;

    let _setup = utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 1_000, 1).await;

    let (shuttle_ephemeral_ata, _) = ShuttleMetadata::find_pda(&owner, &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle_ephemeral_ata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_ephemeral_ata, mint);

    let mut init_data = instruction::ESplInstruction::InitializeShuttleEphemeralAta.to_vec();
    init_data.extend_from_slice(&shuttle_id.to_le_bytes());

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
        data: init_data,
    };

    let tx_init =
        Transaction::new_signed_with_payer(&[ix_init_shuttle], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let shuttle_meta_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(
        shuttle_meta_account.data.len(),
        ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata::LEN
    );
    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata account must exist");
    assert_eq!(shuttle_eata_account.data.len(), EphemeralAta::LEN);

    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(&shuttle_eata, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&shuttle_eata);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&shuttle_eata);

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::DelegateShuttleEphemeralAta
            .with(&DelegateShuttleArgs { validator: None })
            .unwrap(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "del_shuttle_eata::delegate")
        .await
        .unwrap();

    let redelegate_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();

    let tx_redelegate =
        Transaction::new_signed_with_payer(&[ix_delegate], Some(&payer), &[&payer_kp], redelegate_blockhash);
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_redelegate,
        "del_shuttle_eata::redelegate",
    )
    .await
    .unwrap();

    let shuttle_meta_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(shuttle_meta_account.owner, PROGRAM);

    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata must exist");
    assert_eq!(
        shuttle_eata_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
}
