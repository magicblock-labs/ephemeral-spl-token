use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, header_len, item_len, TransferQueueHeader, QUEUE_SEED,
    TRANSFER_QUEUE_VERSION,
};
use solana_account::Account as SolanaAccount;
use solana_instruction::Instruction;
use {
    ephemeral_spl_api::instruction,
    solana_instruction::AccountMeta,
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= header_len());
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

#[tokio::test]
async fn initialize_transfer_queue_default_size() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    let mint = Pubkey::new_unique();
    pt.add_account(
        mint,
        SolanaAccount {
            lamports: 1,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    let context = &mut pt.start_with_context().await;
    let payer = context.payer.pubkey();

    let (queue, bump) = Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref()], &PROGRAM);

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(queue, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, PROGRAM);
    assert_eq!(queue_account.data.len(), 9_728);
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

#[tokio::test]
async fn initialize_transfer_queue_custom_size_is_idempotent() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    let mint = Pubkey::new_unique();
    pt.add_account(
        mint,
        SolanaAccount {
            lamports: 1,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    let context = &mut pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let (queue, bump) = Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref()], &PROGRAM);

    let custom_size = (header_len() + (item_len() * 4)) as u32;
    let mut data = vec![instruction::INITIALIZE_TRANSFER_QUEUE];
    data.extend_from_slice(&custom_size.to_le_bytes());

    let ix_init_custom = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(queue, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data,
    };

    let tx_custom = Transaction::new_signed_with_payer(
        &[ix_init_custom],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_custom)
        .await
        .unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let ix_noop = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(queue, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };
    let tx_noop = Transaction::new_signed_with_payer(
        &[ix_noop],
        Some(&payer),
        &[&context.payer],
        second_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_noop)
        .await
        .unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist after idempotent init");

    assert_eq!(queue_account.data.len(), custom_size as usize);
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
