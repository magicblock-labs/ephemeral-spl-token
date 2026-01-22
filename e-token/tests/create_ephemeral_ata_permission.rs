use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::bpf_loader;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn create_ephemeral_ata_permission() {
    let permission_program_bytes: [u8; 32] =
        ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID
            .as_ref()
            .try_into()
            .unwrap();
    let permission_program_id = Pubkey::new_from_array(permission_program_bytes);

    let mut program_test = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    program_test.prefer_bpf(true);
    let data = read_file("tests/fixtures/acl.so");
    program_test.add_account(
        permission_program_id,
        solana_account::Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );
    let context = program_test.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer;
    let mint = Pubkey::new_unique();

    let (ephemeral_ata, bump) =
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
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump],
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
