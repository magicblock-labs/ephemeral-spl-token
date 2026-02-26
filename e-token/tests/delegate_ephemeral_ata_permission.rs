use ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID;
use ephemeral_spl_api::instruction;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::setup_program_test;

#[tokio::test]
async fn delegate_ephemeral_ata_permission_succeeds() {
    let mut pt = setup_program_test();

    let validator = Pubkey::new_unique();
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

    let context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer;
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) = Pubkey::find_program_address(
        &[user.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &ephemeral_spl_api::program::ID,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &PERMISSION_PROGRAM_ID,
    );

    let ix_init_ata = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump],
    };

    let ix_create_permission = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, bump, flag]
        },
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_create_permission],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let (buffer_pda, _) = Pubkey::find_program_address(
        &[b"buffer", permission_pda.as_ref()],
        &PERMISSION_PROGRAM_ID,
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_delegate_permission = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(validator, false),
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA_PERMISSION, bump],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate_permission],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_delegate)
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
    let mut pt = setup_program_test();

    let validator = Pubkey::new_unique();
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

    let context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = Pubkey::new_unique();
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) = Pubkey::find_program_address(
        &[user.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &ephemeral_spl_api::program::ID,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &PERMISSION_PROGRAM_ID,
    );

    let ix_init_ata = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump],
    };

    let ix_create_permission = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, bump, flag]
        },
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_create_permission],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let (buffer_pda, _) = Pubkey::find_program_address(
        &[b"buffer", permission_pda.as_ref()],
        &PERMISSION_PROGRAM_ID,
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_delegate_permission = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(validator, false),
        ],
        data: vec![instruction::DELEGATE_EPHEMERAL_ATA_PERMISSION, bump],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate_permission],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_delegate)
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
