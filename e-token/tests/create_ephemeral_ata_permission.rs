use ephemeral_spl_api::instruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::pubkey::Pubkey;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::setup_program_test;

#[tokio::test]
async fn create_ephemeral_ata_permission() {
    let permission_program_bytes: [u8; 32] =
        ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID
            .as_ref()
            .try_into()
            .unwrap();
    let permission_program_id = Pubkey::new_from_array(permission_program_bytes);

    let pt = setup_program_test();
    let context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer;
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) = Pubkey::find_program_address(
        &[user.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &permission_program_id,
    );

    let ix_init = Instruction {
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
            AccountMeta::new_readonly(permission_program_id, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, bump, flag]
        },
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_init, ix_create_permission],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    assert_eq!(permission_account.owner, permission_program_id);
    assert!(permission_account.lamports > 0);
}

#[tokio::test]
async fn create_ephemeral_ata_permission_permissionless_default() {
    let permission_program_bytes: [u8; 32] =
        ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID
            .as_ref()
            .try_into()
            .unwrap();
    let permission_program_id = Pubkey::new_from_array(permission_program_bytes);

    let pt = setup_program_test();
    let context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = Pubkey::new_unique();
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) = Pubkey::find_program_address(
        &[user.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &permission_program_id,
    );

    let ix_init = Instruction {
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
            AccountMeta::new_readonly(permission_program_id, false),
        ],
        data: {
            let flag =
                ephemeral_rollups_pinocchio::acl::types::MemberFlags::default().to_acl_flag_byte();
            vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, bump, flag]
        },
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_init, ix_create_permission],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    assert_eq!(permission_account.owner, permission_program_id);
    assert!(permission_account.lamports > 0);
}
