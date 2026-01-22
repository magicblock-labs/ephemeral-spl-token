use ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::bpf_loader;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn delegate_ephemeral_ata_permission_succeeds() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let acl_data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        PERMISSION_PROGRAM_ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(acl_data.len()).max(1),
            data: acl_data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    let dlp_data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(dlp_data.len()).max(1),
            data: dlp_data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

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
        &PROGRAM,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &PERMISSION_PROGRAM_ID.into(),
    );

    let ix_init_ata = Instruction {
        program_id: PROGRAM,
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
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID.into(), false),
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
        &PERMISSION_PROGRAM_ID.into(),
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );

    let ix_delegate_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID.into(), false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID.into(), false),
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
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into()
    );
}

#[tokio::test]
async fn delegate_ephemeral_ata_permission_non_owner_succeeds() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let acl_data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        PERMISSION_PROGRAM_ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(acl_data.len()).max(1),
            data: acl_data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    let dlp_data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(dlp_data.len()).max(1),
            data: dlp_data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

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
        &PROGRAM,
    );
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &PERMISSION_PROGRAM_ID.into(),
    );

    let ix_init_ata = Instruction {
        program_id: PROGRAM,
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
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID.into(), false),
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
        &PERMISSION_PROGRAM_ID.into(),
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into(),
    );

    let ix_delegate_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID.into(), false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID.into(), false),
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
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID.into()
    );
}
