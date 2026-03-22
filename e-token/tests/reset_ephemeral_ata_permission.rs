use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

fn find_member_flag(data: &[u8], member_pubkey: &Pubkey, expected: u8) -> Option<u8> {
    let key_bytes = member_pubkey.as_ref();
    if data.len() < key_bytes.len() {
        return None;
    }
    for i in 0..=data.len() - key_bytes.len() {
        if &data[i..i + key_bytes.len()] == key_bytes {
            if i > 0 && data[i - 1] == expected {
                return Some(data[i - 1]);
            }
            if i + key_bytes.len() < data.len() && data[i + key_bytes.len()] == expected {
                return Some(data[i + key_bytes.len()]);
            }
        }
    }
    None
}

#[tokio::test]
async fn reset_ephemeral_ata_permission() {
    let permission_program_id = utils::permission_program_id();

    let context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let user = payer;
    let mint = utils::test_pubkey("reset_ephemeral_ata_permission::mint");

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

    let reset_flag = 0;
    let ix_reset_permission = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new(permission_pda, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(permission_program_id, false),
        ],
        data: vec![instruction::RESET_EPHEMERAL_ATA_PERMISSION, reset_flag],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_init, ix_create_permission, ix_reset_permission],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "reset_eata_perm::reset",
    )
    .await
    .unwrap();

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    let mut expected_flags =
        ephemeral_rollups_pinocchio::acl::types::MemberFlags::from_acl_flag_byte(reset_flag);
    expected_flags.set(ephemeral_rollups_pinocchio::acl::types::MemberFlags::AUTHORITY);
    let member_flag = find_member_flag(&permission_account.data, &payer, expected_flags.as_u8())
        .expect("permission data must contain member flags for owner");
    assert_eq!(member_flag, expected_flags.as_u8());
}
