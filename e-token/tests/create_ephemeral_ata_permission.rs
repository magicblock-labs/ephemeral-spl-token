use ephemeral_spl_api::instruction;
use ephemeral_spl_api::ID as PROGRAM;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

#[tokio::test]
async fn create_ephemeral_ata_permission() {
    let permission_program_id = utils::permission_program_id();

    let context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer;
    let mint = utils::test_pubkey("create_ephemeral_ata_permission::mint");

    let (ephemeral_ata, _) =
        Pubkey::find_program_address(&[user.as_ref(), mint.as_ref()], &PROGRAM);
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &permission_program_id,
    );

    let ix_init = Instruction {
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

    let tx = Transaction::new_signed_with_payer(
        &[ix_init, ix_create_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "create_eata_perm::default",
    )
    .await
    .unwrap();

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
    let permission_program_id = utils::permission_program_id();

    let context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = utils::test_pubkey("create_ephemeral_ata_permission_permissionless_default::user");
    let mint = utils::test_pubkey("create_ephemeral_ata_permission_permissionless_default::mint");

    let (ephemeral_ata, _) =
        Pubkey::find_program_address(&[user.as_ref(), mint.as_ref()], &PROGRAM);
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &permission_program_id,
    );

    let ix_init = Instruction {
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

    let tx = Transaction::new_signed_with_payer(
        &[ix_init, ix_create_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "create_eata_perm::permissionless",
    )
    .await
    .unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    assert_eq!(permission_account.owner, permission_program_id);
    assert!(permission_account.lamports > 0);
}
