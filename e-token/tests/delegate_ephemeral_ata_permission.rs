use ephemeral_rollups_pinocchio::{
    acl::permission_pda_from_permissioned_account,
    pda::{
        delegate_buffer_pda_from_delegated_account_and_owner_program,
        delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account,
    },
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::ID as PROGRAM;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

#[tokio::test]
async fn delegate_ephemeral_ata_permission_succeeds() {
    let permission_program_id = utils::permission_program_id();
    let validator = utils::test_pubkey("delegate_ephemeral_ata_permission_succeeds::validator");

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            validator,
            Account {
                lamports: Rent::default().minimum_balance(0).max(1),
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer;
    let mint = utils::test_pubkey("delegate_ephemeral_ata_permission_succeeds::mint");

    let (ephemeral_ata, _) = EphemeralAta::find_pda(&user, &mint);
    let permission_pda = permission_pda_from_permissioned_account(&ephemeral_ata);

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

    let ix_create_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(permission_program_id, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, flag]
        },
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_create_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(
        &permission_pda,
        &permission_program_id,
    );
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&permission_pda);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&permission_pda);

    let ix_delegate_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(permission_program_id, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(validator, false),
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA_PERMISSION],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate_permission.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        "del_eata_perm::delegate",
    )
    .await
    .unwrap();

    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx_redelegate = Transaction::new_signed_with_payer(
        &[ix_delegate_permission],
        Some(&payer),
        &[&payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_redelegate,
        "del_eata_perm::redelegate",
    )
    .await
    .unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");
    assert_eq!(
        permission_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
}

#[tokio::test]
async fn delegate_ephemeral_ata_permission_non_owner_succeeds() {
    let permission_program_id = utils::permission_program_id();
    let validator =
        utils::test_pubkey("delegate_ephemeral_ata_permission_non_owner_succeeds::validator");

    let context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            validator,
            Account {
                lamports: Rent::default().minimum_balance(0).max(1),
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = utils::test_pubkey("delegate_ephemeral_ata_permission_non_owner_succeeds::user");
    let mint = utils::test_pubkey("delegate_ephemeral_ata_permission_non_owner_succeeds::mint");

    let (ephemeral_ata, _) = EphemeralAta::find_pda(&user, &mint);
    let permission_pda = permission_pda_from_permissioned_account(&ephemeral_ata);

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

    let ix_create_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(permission_program_id, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, flag]
        },
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_create_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(
        &permission_pda,
        &permission_program_id,
    );
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&permission_pda);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&permission_pda);

    let ix_delegate_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(permission_program_id, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(validator, false),
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA_PERMISSION],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        "del_eata_perm::non_owner",
    )
    .await
    .unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");
    assert_eq!(
        permission_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
}
