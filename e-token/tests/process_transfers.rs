use crate::utils::initialize_transfer_queue;
use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::load_unchecked;
use ephemeral_spl_api::state::transfer_queue::{QueuedTransfer, TransferQueue};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::clock::Clock;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;

use {solana_program_test::tokio, solana_signer::Signer, solana_transaction::Transaction};

mod utils;

use crate::utils::setup_program_test;

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
async fn test_process_transfers() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let user = payer; // in this test, user == payer

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive PDAs and setup mint/accounts via utils
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    let user_ata = setup.user_tokens[0];
    let recipient_ata = setup.recipient_tokens[0];
    let queue_shuttle_id = 0;
    let pdas = utils::derive_pdas(
        ephemeral_spl_api::program::ID,
        payer,
        mint,
        queue_shuttle_id,
    );

    // Assert initial SPL token balances
    let user_token_acc_before = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state_before = Account::unpack(&user_token_acc_before.data).unwrap();
    assert_eq!(user_token_state_before.amount, STARTING_BALANCE);

    initialize_transfer_queue(&mut context, payer, mint, queue_shuttle_id, &pdas).await;

    // Queue transfer
    let amount: u64 = 100 * 10u64.pow(DECIMALS as u32);
    let chunk_size: u64 = 10 * 10u64.pow(DECIMALS as u32);
    let interval_seconds: u16 = 10;
    let n_chunks = amount.div_ceil(chunk_size);
    let mut data = vec![instruction::QUEUE_TRANSFER];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&chunk_size.to_le_bytes());
    data.extend_from_slice(&interval_seconds.to_le_bytes());

    context.set_sysvar(&Clock {
        slot: 0,
        epoch_start_timestamp: 0,
        epoch: 0,
        leader_schedule_epoch: 0,
        unix_timestamp: 0,
    });

    let ix_queue_transfer = Instruction {
        program_id: ephemeral_spl_api::program::ID,
        accounts: vec![
            AccountMeta::new_readonly(mint, false), // [] Mint pubkey (seed/consistency)
            AccountMeta::new_readonly(user, true),  // [signer] user source token acc
            AccountMeta::new(user_ata, false),      // [writable] user source token acc
            AccountMeta::new(pdas.queue, false),    // [writable] queue token acc
            AccountMeta::new(pdas.queue_ata, false), // [writable] queue token acc
            AccountMeta::new_readonly(recipient_ata, false), // [] recipient token acc
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program id (readonly)
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_queue_transfer],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    for i in 0..n_chunks {
        let queue_pda_before = context
            .banks_client
            .get_account(pdas.queue)
            .await
            .unwrap()
            .expect("queue pda account must exist");
        let queue_pda_state_before =
            unsafe { load_unchecked::<TransferQueue>(queue_pda_before.data.as_slice()).unwrap() };
        eprintln!(
            "Queue Before:\n{}",
            format!("{:?}", queue_pda_state_before.queue[0])
        );
        assert_eq!(queue_pda_state_before.length, 1);
        let expected_transfer = QueuedTransfer {
            amount: amount - i * chunk_size,
            chunk_size,
            interval_seconds,
            source: user_ata,
            destination: recipient_ata,
            last_transfer: (i * interval_seconds as u64) as i64,
        };
        assert_eq!(queue_pda_state_before.queue[0], expected_transfer);

        let queue_ata_before = context
            .banks_client
            .get_account(pdas.queue_ata)
            .await
            .unwrap()
            .expect("queue ata account must exist");
        let queue_ata_state_before = Account::unpack(&queue_ata_before.data).unwrap();
        assert_eq!(queue_ata_state_before.amount, amount - i * chunk_size);

        let shuttle_ata_before = context
            .banks_client
            .get_account(pdas.queue_shuttle_ata)
            .await
            .unwrap()
            .expect("shuttle ata account must exist");
        let shuttle_ata_state_before = Account::unpack(&shuttle_ata_before.data).unwrap();
        assert_eq!(shuttle_ata_state_before.amount, i * chunk_size);

        context.set_sysvar(&Clock {
            slot: 0,
            epoch_start_timestamp: 0,
            epoch: 0,
            leader_schedule_epoch: 0,
            unix_timestamp: ((i + 1) * interval_seconds as u64) as i64,
        });
        let ix_process_transfers = Instruction::new_with_bytes(
            ephemeral_spl_api::program::ID,
            &vec![instruction::PROCESS_TRANSFERS, pdas.queue_shuttle_eata_bump],
            vec![
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
        );
        let blockhash = context.get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix_process_transfers],
            Some(&payer),
            &[&context.payer],
            blockhash,
        );
        context.banks_client.process_transaction(tx).await.unwrap();

        let queue_ata_after = context
            .banks_client
            .get_account(pdas.queue_ata)
            .await
            .unwrap()
            .expect("queue ata account must exist");
        let queue_ata_state_after = Account::unpack(&queue_ata_after.data).unwrap();
        assert_eq!(
            queue_ata_state_after.amount,
            queue_ata_state_before.amount - chunk_size
        );

        let shuttle_ata_after = context
            .banks_client
            .get_account(pdas.queue_shuttle_ata)
            .await
            .unwrap()
            .expect("shuttle ata account must exist");
        let shuttle_ata_state_after = Account::unpack(&shuttle_ata_after.data).unwrap();
        assert_eq!(
            shuttle_ata_state_after.amount,
            shuttle_ata_state_before.amount + chunk_size
        );
    }

    let queue_pda_after = context
        .banks_client
        .get_account(pdas.queue)
        .await
        .unwrap()
        .expect("queue pda account must exist");
    let queue_pda_state_after =
        unsafe { load_unchecked::<TransferQueue>(queue_pda_after.data.as_slice()).unwrap() };
    assert_eq!(queue_pda_state_after.length, 0);
}
