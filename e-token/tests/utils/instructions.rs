use ephemeral_rollups_pinocchio::acl::PERMISSION_PROGRAM_ID;
use ephemeral_spl_api::{
    instruction,
    state::{transfer_queue::TransferQueue, RawType},
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::ProgramTestContext;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::utils::{associated_token_program_id, Pdas};

pub async fn allocate_transfer_queue(
    context: &mut ProgramTestContext,
    mint: Pubkey,
    queue_pda: Pubkey,
) {
    let ixs = (0..(TransferQueue::LEN.div_ceil(10240)))
        .map(|_| {
            Instruction::new_with_bytes(
                ephemeral_spl_api::program::ID,
                &vec![instruction::ALLOCATE_QUEUE],
                vec![
                    AccountMeta::new(context.payer.pubkey(), true),
                    AccountMeta::new(queue_pda, false),
                    AccountMeta::new_readonly(mint, false),
                    AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                ],
            )
        })
        .collect::<Vec<_>>();

    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();
}

pub async fn initialize_transfer_queue(
    context: &mut ProgramTestContext,
    payer: Pubkey,
    mint: Pubkey,
    queue_shuttle_id: u32,
    pdas: &Pdas,
) {
    allocate_transfer_queue(context, mint, pdas.queue).await;

    let ix_initialize_transfer_queue = Instruction::new_with_bytes(
        ephemeral_spl_api::program::ID,
        &vec![
            vec![instruction::INITIALIZE_TRANSFER_QUEUE, pdas.queue_eata_bump],
            queue_shuttle_id.to_le_bytes().to_vec(),
        ]
        .concat(),
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(pdas.queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(pdas.queue_permission_pda, false),
            AccountMeta::new(pdas.queue_ata, false),
            AccountMeta::new(pdas.queue_eata, false),
            AccountMeta::new(pdas.queue_eata_permission_pda, false),
            AccountMeta::new(pdas.queue_shuttle, false),
            AccountMeta::new(pdas.queue_shuttle_ata, false),
            AccountMeta::new(pdas.queue_shuttle_eata, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(associated_token_program_id(), false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
    );

    let tx = Transaction::new_signed_with_payer(
        &[ix_initialize_transfer_queue],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();
}
