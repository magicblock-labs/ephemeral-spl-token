mod utils;

use ephemeral_rollups_pinocchio::acl::{consts::PERMISSION_PROGRAM_ID, Permission};
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, Member, MemberFlags,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::transfer_queue::QueuedTransfer;
use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use ephemeral_spl_api::state::{load_unchecked, RawType};
use pinocchio_token_2022::state::TokenAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

use crate::utils::{
    allocate_transfer_queue, associated_token_program_id, derive_associated_token_address,
    setup_program_test,
};

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000;

#[tokio::test]
async fn initialize_transfer_queue() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.insecure_clone();
    let payer_pubkey = payer.pubkey();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let (queue_pda, queue_bump) = TransferQueue::find_pda(&mint);
    let (queue_eata_pda, queue_eata_bump) = Pubkey::find_program_address(
        &[queue_pda.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    let queue_permission_pda = permission_pda_from_permissioned_account(&queue_pda);
    let queue_ata_pda = derive_associated_token_address(&queue_pda, &mint);
    let queue_eata_permission_pda = permission_pda_from_permissioned_account(&queue_eata_pda);
    let shuttle_id = 0;
    let (shuttle_pda, _shuttle_bump) = ShuttleEphemeralAta::find_pda(&queue_pda, &mint, shuttle_id);
    let shuttle_ata_pda = derive_associated_token_address(&shuttle_pda, &mint);
    let (shuttle_eata_pda, _shuttle_eata_bump) = Pubkey::find_program_address(
        &[shuttle_pda.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );

    // Setup mint/accounts via utils
    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer_pubkey,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;

    allocate_transfer_queue(&mut context, mint, queue_pda).await;

    let ix_initialize_transfer_queue = Instruction::new_with_bytes(
        ephemeral_spl_api::program::ID,
        &vec![
            vec![instruction::INITIALIZE_TRANSFER_QUEUE, queue_eata_bump],
            shuttle_id.to_le_bytes().to_vec(),
        ]
        .concat(),
        vec![
            AccountMeta::new(payer_pubkey, true),
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

    let tx = Transaction::new_signed_with_payer(
        &[ix_initialize_transfer_queue],
        Some(&Pubkey::new_from_array(payer_pubkey.to_bytes())),
        &[&payer],
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    let queue_account = context
        .banks_client
        .get_account(queue_pda)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_account.data.len(), TransferQueue::LEN);
    let queue = unsafe { load_unchecked::<TransferQueue>(queue_account.data.as_slice()).unwrap() };
    assert_eq!(queue.mint, mint);
    assert_eq!(queue.bump, queue_bump);
    assert_eq!(queue.length, 0);
    assert_eq!(
        queue.queue,
        core::array::from_fn(|_| QueuedTransfer::default())
    );

    let queue_permission_account = context
        .banks_client
        .get_account(queue_permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    assert_eq!(queue_permission_account.owner, PERMISSION_PROGRAM_ID);
    let permission = Permission::try_from_slice(&queue_permission_account.data).unwrap();
    assert_eq!(permission.permissioned_account, queue_pda);
    let expected_members = [Member {
        flags: MemberFlags::default(),
        pubkey: ephemeral_spl_api::program::ID,
    }];
    assert_eq!(permission.members, Some(expected_members.as_ref()));

    let queue_ata_account = context
        .banks_client
        .get_account(queue_ata_pda)
        .await
        .unwrap()
        .expect("queue ata account must exist");

    assert_eq!(queue_ata_account.owner, spl_token_interface::ID);
    let queue_ata =
        unsafe { TokenAccount::from_bytes_unchecked(queue_ata_account.data.as_slice()) };
    assert_eq!(queue_ata.mint(), &mint);
    assert_eq!(queue_ata.amount(), 0);
    assert_eq!(queue_ata.owner(), &queue_pda);

    let queue_eata_account = context
        .banks_client
        .get_account(queue_eata_pda)
        .await
        .unwrap()
        .expect("queue eata account must exist");

    assert_eq!(queue_eata_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_eata_account.data.len(), EphemeralAta::LEN);
    let queue_eata =
        unsafe { load_unchecked::<EphemeralAta>(queue_eata_account.data.as_slice()).unwrap() };
    assert_eq!(queue_eata.mint, mint);
    assert_eq!(queue_eata.amount, 0);
    assert_eq!(queue_eata.owner, queue_pda);

    let queue_eata_permission_account = context
        .banks_client
        .get_account(queue_eata_permission_pda)
        .await
        .unwrap()
        .expect("queue eata permission account must exist");

    assert_eq!(queue_eata_permission_account.owner, PERMISSION_PROGRAM_ID);
    let queue_eata_permission =
        Permission::try_from_slice(&queue_eata_permission_account.data).unwrap();
    assert_eq!(queue_eata_permission.permissioned_account, queue_eata_pda);
    let expected_members = [Member {
        flags: MemberFlags::default(),
        pubkey: ephemeral_spl_api::program::ID,
    }];
    assert_eq!(
        queue_eata_permission.members,
        Some(expected_members.as_ref())
    );

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_pda)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(shuttle_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(shuttle_account.data.len(), ShuttleEphemeralAta::LEN);
    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_account.data.as_slice()).unwrap() };
    assert_eq!(shuttle.owner, queue_pda);
    assert_eq!(shuttle.payer, payer_pubkey);
    assert_eq!(shuttle.id, 0);

    let shuttle_ata_account = context
        .banks_client
        .get_account(shuttle_ata_pda)
        .await
        .unwrap()
        .expect("shuttle ata account must exist");
    assert_eq!(shuttle_ata_account.owner, spl_token_interface::ID);
    let shuttle_ata =
        unsafe { TokenAccount::from_bytes_unchecked(shuttle_ata_account.data.as_slice()) };
    assert_eq!(shuttle_ata.mint(), &mint);
    assert_eq!(shuttle_ata.amount(), 0);
    assert_eq!(shuttle_ata.owner(), &shuttle_pda);
}
