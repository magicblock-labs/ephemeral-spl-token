use crate::utils::initialize_transfer_queue;
use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::instruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;

use {solana_program_test::tokio, solana_signer::Signer, solana_transaction::Transaction};

mod utils;

use crate::utils::setup_program_test;

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
#[ignore]
async fn test_start_shuttle_crank() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive PDAs and setup mint/accounts via utils
    utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let queue_shuttle_id = 0;
    let pdas = utils::derive_pdas(
        ephemeral_spl_api::program::ID,
        payer,
        mint,
        queue_shuttle_id,
    );

    initialize_transfer_queue(&mut context, payer, mint, queue_shuttle_id, &pdas).await;

    let crank_id = 123_i64;
    let ix_process_transfers = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        data: vec![
            vec![instruction::START_SHUTTLE_CRANK],
            crank_id.to_le_bytes().to_vec(),
            vec![pdas.queue_shuttle_eata_bump],
        ]
        .concat(),
        accounts: vec![
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(pdas.queue, false),
            AccountMeta::new(pdas.queue_ata, false),
            AccountMeta::new(pdas.queue_shuttle, false),
            AccountMeta::new(pdas.queue_shuttle_ata, false),
            AccountMeta::new(pdas.queue_shuttle_eata, false),
            AccountMeta::new(MAGIC_CONTEXT_ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix_process_transfers],
        Some(&payer),
        &[&context.payer],
        blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
}
