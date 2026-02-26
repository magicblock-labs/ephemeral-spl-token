use crate::utils::{allocate_transfer_queue, associated_token_program_id};
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::load_unchecked;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::transfer_queue::{QueuedTransfer, TransferQueue};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::clock::Clock;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;

use {
    solana_program_test::tokio, solana_pubkey::Pubkey, solana_signer::Signer,
    solana_transaction::Transaction,
};

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
    let (queue_pda, _queue_bump) = TransferQueue::find_pda(&mint);
    let queue_permission_pda = permission_pda_from_permissioned_account(&queue_pda);
    let queue_ata_pda = utils::derive_associated_token_address(&queue_pda, &mint);
    let (queue_eata_pda, queue_eata_bump) = Pubkey::find_program_address(
        &[queue_pda.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    let queue_eata_permission_pda = permission_pda_from_permissioned_account(&queue_eata_pda);
    let shuttle_id = 0;
    let (shuttle_pda, _shuttle_bump) = ShuttleEphemeralAta::find_pda(&queue_pda, &mint, shuttle_id);
    let shuttle_ata_pda = utils::derive_associated_token_address(&shuttle_pda, &mint);
    let (shuttle_eata_pda, _shuttle_eata_bump) = Pubkey::find_program_address(
        &[shuttle_pda.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
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

    allocate_transfer_queue(&mut context, mint, queue_pda).await;

    // Initialize Transfer Queue
    let ix_initialize_transfer_queue = Instruction::new_with_bytes(
        ephemeral_spl_api::program::ID,
        &vec![
            vec![instruction::INITIALIZE_TRANSFER_QUEUE, queue_eata_bump],
            shuttle_id.to_le_bytes().to_vec(),
        ]
        .concat(),
        vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue_pda, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(queue_permission_pda, false),
            AccountMeta::new(queue_ata_pda, false),
            AccountMeta::new(queue_eata_pda, false),
            AccountMeta::new(queue_eata_permission_pda, false),
            AccountMeta::new(shuttle_pda, false),
            AccountMeta::new(shuttle_ata_pda, false),
            AccountMeta::new(shuttle_eata_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(associated_token_program_id(), false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
    );

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
            AccountMeta::new(queue_pda, false),     // [writable] queue token acc
            AccountMeta::new(queue_ata_pda, false), // [writable] queue token acc
            AccountMeta::new_readonly(recipient_ata, false), // [] recipient token acc
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program id (readonly)
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_initialize_transfer_queue, ix_queue_transfer],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    for i in 0..n_chunks {
        let queue_pda_before = context
            .banks_client
            .get_account(queue_pda)
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
            .get_account(queue_ata_pda)
            .await
            .unwrap()
            .expect("queue ata account must exist");
        let queue_ata_state_before = Account::unpack(&queue_ata_before.data).unwrap();
        assert_eq!(queue_ata_state_before.amount, amount - i * chunk_size);

        let shuttle_ata_before = context
            .banks_client
            .get_account(shuttle_ata_pda)
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
            &vec![
                vec![instruction::PROCESS_TRANSFERS],
                shuttle_id.to_le_bytes().to_vec(),
            ]
            .concat(),
            vec![
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(queue_pda, false),
                AccountMeta::new(queue_ata_pda, false),
                AccountMeta::new(shuttle_pda, false),
                AccountMeta::new(shuttle_ata_pda, false),
                AccountMeta::new(shuttle_eata_pda, false),
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
            .get_account(queue_ata_pda)
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
            .get_account(shuttle_ata_pda)
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
        .get_account(queue_pda)
        .await
        .unwrap()
        .expect("queue pda account must exist");
    let queue_pda_state_after =
        unsafe { load_unchecked::<TransferQueue>(queue_pda_after.data.as_slice()).unwrap() };
    assert_eq!(queue_pda_state_after.length, 0);
}
