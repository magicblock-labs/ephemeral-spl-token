use ephemeral_spl_api::{
    instruction,
    state::transfer_queue::{queue_views_checked, TransferQueue, HEADER_LEN, ITEM_LEN, TRANSFER_QUEUE_VERSION},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

#[tokio::test]
async fn allocate_transfer_queue_succeeds_and_is_idempotent() {
    let mut context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair("allocate_transfer_queue_succeeds_and_is_idempotent::mint");
    let mint = mint_kp.pubkey();
    utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 0, 1).await;

    let validator = Keypair::new().pubkey();
    let (queue, bump) = TransferQueue::find_pda(&mint, &validator);

    const N_ITEMS: usize = 9999;
    let ix_init_queue = utils::build_initialize_transfer_queue_ix(
        payer,
        queue,
        mint,
        validator,
        Some(N_ITEMS as u32),
        spl_token_interface::ID,
    );

    let tx_init =
        Transaction::new_signed_with_payer(&[ix_init_queue], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let ix_allocate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::AllocateTransferQueue.to_vec(),
    };

    let mut previous_data_len = 0usize;
    let mut final_data_len = None;
    // Splitting into chunks of 10 to avoid MaxInstructionTraceLengthExceeded
    for i in 0..256 {
        let blockhash = context.get_new_latest_blockhash().await.unwrap();
        let batch = vec![ix_allocate.clone(); 10];
        let tx_allocate = Transaction::new_signed_with_payer(&batch, Some(&payer), &[&payer_kp], blockhash);
        if i == 0 {
            common::metrics::process_transaction_record_cu(
                &context.banks_client,
                tx_allocate,
                "tq_alloc::first_allocate",
            )
            .await
            .unwrap();
        } else {
            context.banks_client.process_transaction(tx_allocate).await.unwrap();
        }

        let current_data_len = context
            .banks_client
            .get_account(queue)
            .await
            .unwrap()
            .expect("queue account must exist")
            .data
            .len();
        if current_data_len == previous_data_len {
            final_data_len = Some(current_data_len);
            break;
        }
        previous_data_len = current_data_len;
    }

    let final_data_len = final_data_len.expect("queue allocation must converge");
    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");
    let final_capacity = (final_data_len - HEADER_LEN) / ITEM_LEN;
    assert!(final_capacity >= N_ITEMS);

    let (header, items) = queue_views_checked(&queue_account.data).unwrap();
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(header.mint, mint);
    assert_eq!(header.length, 0);
    assert_eq!(header.validator, validator);

    assert_eq!(items.len(), final_capacity);

    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx_allocate_again = Transaction::new_signed_with_payer(&[ix_allocate], Some(&payer), &[&payer_kp], blockhash);
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_allocate_again,
        "tq_alloc::already_allocated",
    )
    .await
    .unwrap();

    let queue_account_after_extra_allocate = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must still exist");
    assert_eq!(queue_account_after_extra_allocate.data.len(), final_data_len);
}
