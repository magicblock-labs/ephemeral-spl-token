use dlp_api::state::DelegationRecord;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program,
    delegation_metadata_pda_from_delegated_account, delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::transfer_queue::{
    TransferQueue, TransferQueueHeader, HEADER_LEN, TRANSFER_QUEUE_VERSION,
};
use ephemeral_spl_api::ID as PROGRAM;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

pub const VALIDATOR: Pubkey = Pubkey::new_from_array([77; 32]);

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= HEADER_LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

#[tokio::test]
async fn delegate_transfer_queue_succeeds_and_is_idempotent() {
    let mint = utils::test_pubkey("delegate_transfer_queue_succeeds_and_is_idempotent::mint");
    let context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            mint,
            Account {
                lamports: 1,
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
    let (queue, bump) = TransferQueue::find_pda(&mint, &VALIDATOR);
    let queue_permission = permission_pda_from_permissioned_account(&queue);

    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(queue_permission, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(VALIDATOR, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_queue],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let queue_permission_account = context
        .banks_client
        .get_account(queue_permission)
        .await
        .unwrap()
        .expect("queue permission account must exist");

    assert_eq!(queue_permission_account.owner, PERMISSION_PROGRAM_ID);

    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(&queue, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&queue);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&queue);

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::DELEGATE_TRANSFER_QUEUE],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        "del_tq::delegate",
    )
    .await
    .unwrap();

    let redelegate_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_redelegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp],
        redelegate_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_redelegate,
        "del_tq::redelegate",
    )
    .await
    .unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(
        queue_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let delegation_record = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    let delegation_record =
        DelegationRecord::try_from_bytes_with_discriminator(&delegation_record.data)
            .expect("delegation record must deserialize");

    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(
        header.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(header.length, 0);
    assert_eq!(
        &delegation_record.authority.to_bytes(),
        VALIDATOR.as_array()
    );
}
