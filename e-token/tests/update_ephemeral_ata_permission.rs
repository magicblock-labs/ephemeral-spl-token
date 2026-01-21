use ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID;
use ephemeral_rollups_pinocchio::acl::types::MemberFlags;
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
async fn update_ephemeral_ata_permission() {
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
            let flags = MemberFlags::default().to_acl_flags_bytes();
            let mut data = vec![instruction::CREATE_EPHEMERAL_ATA_PERMISSION, bump];
            data.extend_from_slice(&flags);
            data
        },
    };

    let mut updated_flags = MemberFlags::default();
    updated_flags.remove(MemberFlags::TX_LOGS);
    updated_flags.remove(MemberFlags::TX_MESSAGE);
    let ix_update_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID.into(), false),
        ],
        data: {
            let flags = updated_flags.to_acl_flags_bytes();
            let mut data = vec![instruction::UPDATE_EPHEMERAL_ATA_PERMISSION, bump];
            data.extend_from_slice(&flags);
            data
        },
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_create_permission, ix_update_permission],
        Some(&payer),
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
    assert_eq!(permission_account.owner, PERMISSION_PROGRAM_ID.into());
}
