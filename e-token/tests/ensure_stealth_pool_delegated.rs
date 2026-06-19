use ephemeral_rollups_pinocchio::{
    acl::{permission_pda_from_permissioned_account, types::MemberFlags, PERMISSION_PROGRAM_ID},
    pda::{
        delegate_buffer_pda_from_delegated_account_and_owner_program, delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account,
    },
};
use ephemeral_spl_api::{
    instruction,
    instructions::EnsureStealthPoolDelegatedArgs,
    state::{stealth_pool::StealthPool, RawType},
    ID as PROGRAM,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;
use wheels::layout::Encodable as _;

mod common;
mod utils;

fn build_ensure_stealth_pool_delegated_ix(
    payer: solana_pubkey::Pubkey,
    authority: solana_pubkey::Pubkey,
    handle: &[u8],
) -> (solana_pubkey::Pubkey, solana_pubkey::Pubkey, Instruction) {
    let (stealth_pool, _) = StealthPool::find_pda(handle).unwrap();
    let stealth_pool_permission = permission_pda_from_permissioned_account(&stealth_pool);
    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(&stealth_pool, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&stealth_pool);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&stealth_pool);

    let data = instruction::ESplInstruction::EnsureStealthPoolDelegated.with_data(
        &EnsureStealthPoolDelegatedArgs {
            handle: StealthPool::store_handle(handle).unwrap(),
            validator: None,
        }
        .encode()
        .unwrap(),
    );

    (
        stealth_pool,
        stealth_pool_permission,
        Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(stealth_pool, false),
                AccountMeta::new(stealth_pool_permission, false),
                AccountMeta::new_readonly(PROGRAM, false),
                AccountMeta::new(buffer_pda, false),
                AccountMeta::new(delegation_record_pda, false),
                AccountMeta::new(delegation_metadata_pda, false),
                AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data,
        },
    )
}

fn find_member_flag(data: &[u8], member_pubkey: &solana_pubkey::Pubkey, expected: u8) -> Option<u8> {
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
async fn ensure_stealth_pool_delegated_creates_pool_permission() {
    let mut context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let handle = b"ensure-stealth-pool.block";
    let (stealth_pool, stealth_pool_permission, ix) = build_ensure_stealth_pool_delegated_ix(payer, payer, handle);

    let tx = Transaction::new_signed_with_payer(&[ix.clone()], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    let stealth_pool_account = context
        .banks_client
        .get_account(stealth_pool)
        .await
        .unwrap()
        .expect("stealth pool account must exist");
    assert_eq!(
        stealth_pool_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let permission_account = context
        .banks_client
        .get_account(stealth_pool_permission)
        .await
        .unwrap()
        .expect("stealth pool permission account must exist");
    assert_eq!(permission_account.owner, PERMISSION_PROGRAM_ID);
    assert_eq!(
        find_member_flag(&permission_account.data, &payer, MemberFlags::AUTHORITY),
        Some(MemberFlags::AUTHORITY)
    );

    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();
}

#[tokio::test]
async fn ensure_stealth_pool_delegated_creates_permission_when_pool_is_already_delegated() {
    let mut context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let handle = b"ensure-stealth-pool-existing.block";
    let (stealth_pool, stealth_pool_permission, ix) = build_ensure_stealth_pool_delegated_ix(payer, payer, handle);
    let data = vec![0u8; StealthPool::LEN];
    let rent = Rent::default();

    context.set_account(
        &stealth_pool,
        &Account {
            lamports: rent.minimum_balance(data.len()),
            data,
            owner: ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    let permission_account = context
        .banks_client
        .get_account(stealth_pool_permission)
        .await
        .unwrap()
        .expect("stealth pool permission account must exist");
    assert_eq!(permission_account.owner, PERMISSION_PROGRAM_ID);
    assert_eq!(
        find_member_flag(&permission_account.data, &payer, MemberFlags::AUTHORITY),
        Some(MemberFlags::AUTHORITY)
    );
}

#[tokio::test]
async fn ensure_stealth_pool_delegated_supports_255_byte_handles() {
    let context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let handle = [b'a'; StealthPool::MAX_HANDLE_BYTES];
    let (stealth_pool, stealth_pool_permission, ix) = build_ensure_stealth_pool_delegated_ix(payer, payer, &handle);

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    let stealth_pool_account = context
        .banks_client
        .get_account(stealth_pool)
        .await
        .unwrap()
        .expect("stealth pool account must exist");
    assert_eq!(
        stealth_pool_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let permission_account = context
        .banks_client
        .get_account(stealth_pool_permission)
        .await
        .unwrap()
        .expect("stealth pool permission account must exist");
    assert_eq!(permission_account.owner, PERMISSION_PROGRAM_ID);
}
