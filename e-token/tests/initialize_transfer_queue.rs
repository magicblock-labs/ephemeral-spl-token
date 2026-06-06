use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, TransferQueue, TransferQueueHeader, HEADER_LEN, ITEM_LEN,
    TRANSFER_QUEUE_VERSION,
};
use ephemeral_spl_api::ID as PROGRAM;
use {
    solana_program_test::tokio, solana_pubkey::Pubkey, solana_signer::Signer,
    solana_transaction::Transaction,
};

mod common;
mod utils;

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= HEADER_LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

const DEFAULT_TRANSFER_QUEUE_ITEMS: usize = 100;
const VALIDATOR: Pubkey = Pubkey::new_from_array([77; 32]);

#[tokio::test]
async fn initialize_transfer_queue_default_size() {
    let mut context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair("initialize_transfer_queue_default_size::mint");
    let mint = mint_kp.pubkey();
    utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 0, 1).await;

    let (queue, bump) = TransferQueue::find_pda(&mint, &VALIDATOR);
    let ix = utils::build_initialize_transfer_queue_ix(
        payer,
        queue,
        mint,
        VALIDATOR,
        None,
        spl_token_interface::ID,
    );

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "init_tq::default")
        .await
        .unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, PROGRAM);
    assert_eq!(
        queue_account.data.len(),
        HEADER_LEN + ITEM_LEN * DEFAULT_TRANSFER_QUEUE_ITEMS
    );
    assert_eq!(
        capacity_from_data_len(queue_account.data.len()),
        DEFAULT_TRANSFER_QUEUE_ITEMS
    );

    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(
        header.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(header.length, 0);
}

#[tokio::test]
async fn initialize_transfer_queue_custom_size_is_idempotent() {
    let mut context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair("initialize_transfer_queue_custom_size_is_idempotent::mint");
    let mint = mint_kp.pubkey();
    utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 0, 1).await;

    let (queue, bump) = TransferQueue::find_pda(&mint, &VALIDATOR);

    let items = 4_u32;
    let ix_init_custom = utils::build_initialize_transfer_queue_ix(
        payer,
        queue,
        mint,
        VALIDATOR,
        Some(items),
        spl_token_interface::ID,
    );

    let tx_custom = Transaction::new_signed_with_payer(
        &[ix_init_custom],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_custom,
        "init_tq::custom",
    )
    .await
    .unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let ix_noop = utils::build_initialize_transfer_queue_ix(
        payer,
        queue,
        mint,
        VALIDATOR,
        None,
        spl_token_interface::ID,
    );
    let tx_noop = Transaction::new_signed_with_payer(
        &[ix_noop],
        Some(&payer),
        &[&payer_kp],
        second_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx_noop, "init_tq::noop")
        .await
        .unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist after idempotent init");

    assert_eq!(
        queue_account.data.len(),
        HEADER_LEN + ITEM_LEN * items as usize
    );
    assert!(capacity_from_data_len(queue_account.data.len()) >= 1);

    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(
        header.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(header.length, 0);
}
