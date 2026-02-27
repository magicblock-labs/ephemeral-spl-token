mod utils;

use ephemeral_rollups_pinocchio::acl::{consts::PERMISSION_PROGRAM_ID, Permission};
use ephemeral_rollups_pinocchio::acl::{Member, MemberFlags};
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::transfer_queue::QueuedTransfer;
use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use ephemeral_spl_api::state::{load_unchecked, RawType};
use pinocchio_token_2022::state::TokenAccount;
use solana_keypair::Keypair;
use solana_program_test::tokio;
use solana_signer::Signer;

use crate::utils::setup_program_test;

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
    let shuttle_id = 0;

    let pdas = utils::derive_pdas(ephemeral_spl_api::program::ID, payer_pubkey, mint, 0);

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

    utils::initialize_transfer_queue(&mut context, payer_pubkey, mint, shuttle_id, &pdas).await;

    let queue_account = context
        .banks_client
        .get_account(pdas.queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(queue_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_account.data.len(), TransferQueue::LEN);
    let queue = unsafe { load_unchecked::<TransferQueue>(queue_account.data.as_slice()).unwrap() };
    assert_eq!(queue.mint, mint);
    assert_eq!(queue.bump, pdas.queue_bump);
    assert_eq!(queue.length, 0);
    assert_eq!(
        queue.queue,
        core::array::from_fn(|_| QueuedTransfer::default())
    );

    let queue_permission_account = context
        .banks_client
        .get_account(pdas.queue_permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");

    assert_eq!(queue_permission_account.owner, PERMISSION_PROGRAM_ID);
    let permission = Permission::try_from_slice(&queue_permission_account.data).unwrap();
    assert_eq!(permission.permissioned_account, pdas.queue);
    let expected_members = [Member {
        flags: MemberFlags::default(),
        pubkey: ephemeral_spl_api::program::ID,
    }];
    assert_eq!(permission.members, Some(expected_members.as_ref()));

    let queue_ata_account = context
        .banks_client
        .get_account(pdas.queue_ata)
        .await
        .unwrap()
        .expect("queue ata account must exist");

    assert_eq!(queue_ata_account.owner, spl_token_interface::ID);
    let queue_ata =
        unsafe { TokenAccount::from_bytes_unchecked(queue_ata_account.data.as_slice()) };
    assert_eq!(queue_ata.mint(), &mint);
    assert_eq!(queue_ata.amount(), 0);
    assert_eq!(queue_ata.owner(), &pdas.queue);

    let queue_eata_account = context
        .banks_client
        .get_account(pdas.queue_eata)
        .await
        .unwrap()
        .expect("queue eata account must exist");

    assert_eq!(queue_eata_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(queue_eata_account.data.len(), EphemeralAta::LEN);
    let queue_eata =
        unsafe { load_unchecked::<EphemeralAta>(queue_eata_account.data.as_slice()).unwrap() };
    assert_eq!(queue_eata.mint, mint);
    assert_eq!(queue_eata.amount, 0);
    assert_eq!(queue_eata.owner, pdas.queue);

    let queue_eata_permission_account = context
        .banks_client
        .get_account(pdas.queue_eata_permission_pda)
        .await
        .unwrap()
        .expect("queue eata permission account must exist");

    assert_eq!(queue_eata_permission_account.owner, PERMISSION_PROGRAM_ID);
    let queue_eata_permission =
        Permission::try_from_slice(&queue_eata_permission_account.data).unwrap();
    assert_eq!(queue_eata_permission.permissioned_account, pdas.queue_eata);
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
        .get_account(pdas.queue_shuttle)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(shuttle_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(shuttle_account.data.len(), ShuttleEphemeralAta::LEN);
    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_account.data.as_slice()).unwrap() };
    assert_eq!(&shuttle.owner, &pdas.queue);
    assert_eq!(shuttle.payer, payer_pubkey);
    assert_eq!(shuttle.id, 0);

    let shuttle_ata_account = context
        .banks_client
        .get_account(pdas.queue_shuttle_ata)
        .await
        .unwrap()
        .expect("shuttle ata account must exist");
    assert_eq!(shuttle_ata_account.owner, spl_token_interface::ID);
    let shuttle_ata =
        unsafe { TokenAccount::from_bytes_unchecked(shuttle_ata_account.data.as_slice()) };
    assert_eq!(shuttle_ata.mint(), &mint);
    assert_eq!(shuttle_ata.amount(), 0);
    assert_eq!(shuttle_ata.owner(), &pdas.queue_shuttle);
}
