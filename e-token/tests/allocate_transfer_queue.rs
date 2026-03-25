use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    header_len, item_len, queue_views_checked, QUEUE_SEED, TRANSFER_QUEUE_VERSION,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::{tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn allocate_transfer_queue_succeeds_and_is_idempotent() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let mint = Pubkey::new_unique();
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

    let context = &mut pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let validator = Keypair::new().pubkey();
    let (queue, bump) =
        Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref(), validator.as_ref()], &PROGRAM);

    const N_ITEMS: usize = 9999;
    // Overallocate to test idempotency
    const N_ALLOCS: usize = (N_ITEMS * item_len()).div_ceil(10_240) + 10;
    eprintln!("N_ALLOCS: {}", N_ALLOCS);
    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: [
            vec![instruction::INITIALIZE_TRANSFER_QUEUE],
            (N_ITEMS as u32).to_le_bytes().to_vec(),
        ]
        .concat(),
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_queue],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let ix_allocate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::ALLOCATE_TRANSFER_QUEUE],
    };
    // Splitting into chunks of 10 to avoid MaxInstructionTraceLengthExceeded
    for chunk in
        core::array::from_fn::<Instruction, N_ALLOCS, _>(|_| ix_allocate.clone()).chunks(10)
    {
        let blockhash = context.get_new_latest_blockhash().await.unwrap();
        let tx_allocate =
            Transaction::new_signed_with_payer(chunk, Some(&payer), &[&context.payer], blockhash);
        context
            .banks_client
            .process_transaction(tx_allocate)
            .await
            .unwrap();
    }

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(
        (queue_account.data.len() - header_len()) / item_len(),
        N_ITEMS
    );

    let (header, items) = queue_views_checked(&queue_account.data).unwrap();
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(
        header.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(header.length, 0);
    assert_eq!(header.validator, validator);

    assert_eq!(items.len(), N_ITEMS);
}
