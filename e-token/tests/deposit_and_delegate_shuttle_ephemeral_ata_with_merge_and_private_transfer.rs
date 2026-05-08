use std::u64;

use dlp_api::state::DelegationRecord;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program,
    delegation_metadata_pda_from_delegated_account, delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::consts::{
    BASIS_POINTS_FACTOR, PRIVATE_TRANSFER_FEE_BASIS_POINTS,
    SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS, SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::transfer_queue::{TransferQueue, TransferQueueHeader, HEADER_LEN};
use ephemeral_spl_api::state::{load, Initializable};
use ephemeral_spl_api::ID as PROGRAM;
use ephemeral_token_program::{
    DepositAndDelegateShuttleWithPrivateTransferArgs, DepositAndQueueTransferArgs,
    InitializeTransferQueueArgs,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_pack::Pack;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account as SplAccount;

mod common;
mod utils;

const RENT_PDA_SEED: &[u8] = b"rent";
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000 * 10u64.pow(DECIMALS as u32);
const DEPOSIT_AMOUNT: u64 = 100 * 10u64.pow(DECIMALS as u32);
const FEE_AMOUNT: u64 =
    DEPOSIT_AMOUNT * PRIVATE_TRANSFER_FEE_BASIS_POINTS / (BASIS_POINTS_FACTOR as u64);
const MIN_DELAY_MS: u64 = 5_000;
const MAX_DELAY_MS: u64 = 15_000;
const SPLIT: u32 = 4;
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= HEADER_LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

#[tokio::test]
async fn deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_stores_third_action_exact_in(
) {
    deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_stores_third_action(
        false,
    )
    .await;
}

#[tokio::test]
async fn deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_stores_third_action_exact_out(
) {
    deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_stores_third_action(
        true,
    )
    .await;
}

async fn deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_stores_third_action(
    exact_out: bool,
) {
    let owner = utils::test_keypair(
        "deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer::owner",
    );
    let owner_token = utils::test_keypair(
        "deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer::owner_token",
    );

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            owner.pubkey(),
            Account {
                lamports: Rent::default().minimum_balance(0).max(1),
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair(
        "deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer::mint",
    );
    let mint = mint_kp.pubkey();
    let shuttle_id = 9_u32;
    let validator = utils::test_keypair(
        "deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer::validator",
    )
    .pubkey();

    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;
    let destination_owner = payer;

    let (shuttle_metadata, _) = ShuttleMetadata::find_pda(&owner.pubkey(), &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle_metadata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);
    let pdas = utils::derive_pdas(PROGRAM, owner.pubkey(), mint);
    let vault = pdas.vault;
    let (vault_eata, _) = EphemeralAta::find_pda(&vault, &mint);
    let vault_ata = utils::derive_associated_token_address(vault, mint);
    let owner_source_ata = owner_token.pubkey();
    let (queue, _) = TransferQueue::find_pda(&mint, &validator);
    let queue_permission = permission_pda_from_permissioned_account(&queue);
    let ix_init_rent = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeRentPda.to_vec(),
    };
    let ix_fund_rent = transfer(&payer, &rent_pda, 100_000_000);
    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeGlobalVault.to_vec(),
    };
    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(queue_permission, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: instruction::ESplInstruction::InitializeTransferQueue.with_data(
            &InitializeTransferQueueArgs {
                requested_items: None,
            }
            .encode()
            .unwrap(),
        ),
    };
    let rent = context.banks_client.get_rent().await.unwrap();
    let ix_create_owner_source = solana_system_interface::instruction::create_account(
        &payer,
        &owner_source_ata,
        rent.minimum_balance(SplAccount::LEN),
        SplAccount::LEN as u64,
        &spl_token_interface::ID,
    );
    let mut ix_init_owner_source = spl_token_interface::instruction::initialize_account(
        &spl_token_interface::ID,
        &owner_source_ata,
        &mint,
        &owner.pubkey(),
    )
    .unwrap();
    ix_init_owner_source.program_id = spl_token_interface::ID;
    let mut ix_mint_owner_source = spl_token_interface::instruction::mint_to(
        &spl_token_interface::ID,
        &mint,
        &owner_source_ata,
        &payer,
        &[],
        STARTING_BALANCE,
    )
    .unwrap();
    ix_mint_owner_source.program_id = spl_token_interface::ID;

    let tx_init = Transaction::new_signed_with_payer(
        &[
            ix_init_rent,
            ix_fund_rent,
            ix_init_vault,
            ix_init_queue,
            ix_create_owner_source,
            ix_init_owner_source,
            ix_mint_owner_source,
        ],
        Some(&payer),
        &[&payer_kp, &owner_token],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let rent_pda_before = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must exist");

    let buffer_pda =
        delegate_buffer_pda_from_delegated_account_and_owner_program(&shuttle_eata, &PROGRAM);
    let queue_buffer_pda =
        delegate_buffer_pda_from_delegated_account_and_owner_program(&queue, &PROGRAM);
    let queue_delegation_record_pda = delegation_record_pda_from_delegated_account(&queue);
    let queue_delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&queue);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&shuttle_eata);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&shuttle_eata);

    let args = DepositAndDelegateShuttleWithPrivateTransferArgs {
        shuttle_id,
        amount: DEPOSIT_AMOUNT,
        exact_out,
        validator: Some(validator.as_array().to_owned()),
        encrypted_destination: dlp_api::encryption::encrypt_ed25519_recipient(
            destination_owner.as_array(),
            &validator.to_bytes(),
        )
        .expect("validator key should be valid for encryption")
        .try_into()
        .expect("encrypted destination must be 80 bytes"),
        encrypted_data_suffix: dlp_api::encryption::encrypt_ed25519_recipient(
            &DepositAndQueueTransferArgs {
                amount: 0, // dont care its value
                min_delay_ms: MIN_DELAY_MS,
                max_delay_ms: MAX_DELAY_MS,
                split: SPLIT,
                flags: None,
                client_ref_id: None,
            }
            .encode()
            .unwrap()[8..], // except 'amount', encrypt everthing, amount will be prepended by ix.
            &validator.to_bytes(),
        )
        .expect("validator key should be valid for encryption"),
    };

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new(shuttle_metadata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(owner_source_ata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new(queue, false),
        ],
        data: instruction::ESplInstruction::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer.with_data(&args.encode().unwrap()),
    };

    let ix_delegate_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(queue_buffer_pda, false),
            AccountMeta::new(queue_delegation_record_pda, false),
            AccountMeta::new(queue_delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::DelegateTransferQueue.to_vec(),
    };

    let tx_delegate_queue = Transaction::new_signed_with_payer(
        &[ix_delegate_queue],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate_queue,
        &format!(
            "del_shuttle_priv::queue::{}",
            if exact_out { "exact_out" } else { "exact_in" }
        ),
    )
    .await
    .unwrap();

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp, &owner],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        &format!(
            "del_shuttle_priv::shuttle::{}",
            if exact_out { "exact_out" } else { "exact_in" }
        ),
    )
    .await
    .unwrap();

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_metadata)
        .await
        .unwrap()
        .expect("shuttle metadata must exist");
    assert_eq!(shuttle_account.owner, PROGRAM);
    let mut shuttle_data = shuttle_account.data.clone();
    let shuttle = load::<ShuttleMetadata>(shuttle_data.as_mut_slice()).unwrap();
    assert!(shuttle.is_initialized());
    assert_eq!(shuttle.owner.as_array(), &owner.pubkey().to_bytes());
    assert_eq!(shuttle.payer.as_array(), &rent_pda.to_bytes());
    assert_eq!(shuttle.id, shuttle_id);

    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata must exist");
    assert_eq!(
        shuttle_eata_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
    let mut shuttle_eata_data = shuttle_eata_account.data.clone();
    let shuttle_eata_state = load::<EphemeralAta>(shuttle_eata_data.as_mut_slice()).unwrap();

    assert_eq!(
        shuttle_eata_state.amount,
        if exact_out {
            DEPOSIT_AMOUNT + FEE_AMOUNT
        } else {
            DEPOSIT_AMOUNT
        }
    );

    let owner_source_account = context
        .banks_client
        .get_account(owner_source_ata)
        .await
        .unwrap()
        .expect("owner source token account must exist");
    let owner_source_state = SplAccount::unpack(&owner_source_account.data).unwrap();
    assert_eq!(owner_source_state.owner, owner.pubkey());
    assert_eq!(
        owner_source_state.amount,
        if exact_out {
            STARTING_BALANCE - DEPOSIT_AMOUNT - FEE_AMOUNT
        } else {
            STARTING_BALANCE - DEPOSIT_AMOUNT
        }
    );

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue must exist");
    assert_eq!(
        queue_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
    let queue_header = read_header_unaligned(&queue_account.data);
    assert_eq!(queue_header.length, 0);

    let rent_pda_after = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must still exist");
    let delegation_record_account = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    let delegation_metadata_account = context
        .banks_client
        .get_account(delegation_metadata_pda)
        .await
        .unwrap()
        .expect("delegation metadata must exist");

    let record_len = DelegationRecord::size_with_discriminator();
    let record = DelegationRecord::try_from_bytes_with_discriminator(
        &delegation_record_account.data[..record_len],
    )
    .expect("delegation record must deserialize");
    assert_eq!(record.owner.to_bytes(), PROGRAM.to_bytes());
    assert_eq!(
        record.authority.to_bytes(),
        validator.to_bytes(),
        "record.authority = {}, validator = {}",
        record.authority,
        validator
    );
    assert!(
        delegation_record_account.data.len() > record_len,
        "expected stored post-delegation payload bytes"
    );
    let action_payload = &delegation_record_account.data[record_len..];
    let private_transfer_amount = if exact_out {
        DEPOSIT_AMOUNT
    } else {
        DEPOSIT_AMOUNT - FEE_AMOUNT
    };

    let mut private_transfer_prefix =
        instruction::ESplInstruction::DepositAndQueueTransfer.to_vec();
    private_transfer_prefix.extend_from_slice(&private_transfer_amount.to_le_bytes());
    assert!(
        action_payload
            .windows(private_transfer_prefix.len())
            .any(|window| window == private_transfer_prefix.as_slice()),
        "expected stored post-delegation payload to use the net private transfer amount"
    );

    let mut fee_transfer_data = vec![TRANSFER_CHECKED_DISCRIMINATOR];
    fee_transfer_data.extend_from_slice(&FEE_AMOUNT.to_le_bytes());
    fee_transfer_data.push(DECIMALS);
    assert!(
        action_payload
            .windows(fee_transfer_data.len())
            .any(|window| window == fee_transfer_data.as_slice()),
        "expected stored post-delegation payload to include the fee transfer action"
    );

    assert_eq!(
        rent_pda_after.lamports,
        rent_pda_before.lamports
            + SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS
            + SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS
            - shuttle_account.lamports
            - shuttle_eata_account.lamports
            - context
                .banks_client
                .get_account(shuttle_wallet_ata)
                .await
                .unwrap()
                .expect("shuttle wallet ata must exist")
                .lamports
            - delegation_record_account.lamports
            - delegation_metadata_account.lamports
    );
}
