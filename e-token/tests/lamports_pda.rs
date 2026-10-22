use dlp_api::state::DelegationRecord;
use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program, delegation_metadata_pda_from_delegated_account,
    delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::{
    consts::SPONSORED_LAMPORTS_TRANSFER_SETUP_LAMPORTS,
    instruction::{self, ESplInstruction},
    instructions::AmountAndSaltArgs,
    ID as PROGRAM,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_test::{tokio, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::{InstructionError, Transaction, TransactionError};
use wheels::layout::Encodable as _;

use crate::utils::TestInternalInstruction;

mod common;
mod utils;

const RENT_PDA_SEED: &[u8] = b"rent";
const LAMPORTS_PDA_SEED: &[u8] = b"lamports";
const DESTINATION_STARTING_LAMPORTS: u64 = 7;
const TRANSFER_AMOUNT: u64 = 2_500_000;
const SALT: [u8; 32] = [42; 32];
const DUST_LAMPORTS: u64 = 11;
const RENT_PDA_STARTING_LAMPORTS: u64 = 1_000_000;

fn derive_lamports_pda(program: Pubkey, payer: Pubkey, destination: Pubkey, salt: [u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[LAMPORTS_PDA_SEED, payer.as_ref(), destination.as_ref(), salt.as_ref()],
        &program,
    )
}

#[tokio::test]
async fn sponsored_lamports_transfer_delegates_zero_data_pda_and_charges_fee() {
    let validator = utils::test_pubkey("validator");
    let destination =
        utils::test_keypair("sponsored_lamports_transfer_delegates_zero_data_pda_and_charges_fee::destination");
    let destination_delegation_record_pda = delegation_record_pda_from_delegated_account(&destination.pubkey());
    let mut destination_delegation_record_data = vec![0u8; DelegationRecord::size_with_discriminator()];
    DelegationRecord {
        authority: validator.to_bytes().into(),
        owner: solana_system_interface::program::ID.to_bytes().into(),
        delegation_slot: 0,
        lamports: DESTINATION_STARTING_LAMPORTS,
        commit_frequency_ms: 0,
    }
    .to_bytes_with_discriminator(&mut destination_delegation_record_data)
    .unwrap();

    let context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            destination.pubkey(),
            Account {
                lamports: DESTINATION_STARTING_LAMPORTS,
                data: vec![],
                owner: ephemeral_rollups_pinocchio::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            destination_delegation_record_pda,
            Account {
                lamports: Rent::default()
                    .minimum_balance(destination_delegation_record_data.len())
                    .max(1),
                data: destination_delegation_record_data,
                owner: ephemeral_rollups_pinocchio::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let rent = context.banks_client.get_rent().await.unwrap();
    let sponsored_rent = rent.minimum_balance(0);

    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);
    let (lamports_pda, _) = derive_lamports_pda(PROGRAM, payer, destination.pubkey(), SALT);
    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(&lamports_pda, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&lamports_pda);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&lamports_pda);

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
    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_rent, ix_fund_rent],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let rent_pda_before = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must exist");

    let ix_sponsored_transfer = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new(lamports_pda, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(destination.pubkey(), false),
            AccountMeta::new_readonly(destination_delegation_record_pda, false),
        ],
        data: ESplInstruction::SponsoredLamportsTransfer.with_data(
            &AmountAndSaltArgs {
                amount: TRANSFER_AMOUNT,
                salt: SALT,
            }
            .encode()
            .unwrap(),
        ),
    };
    let tx_sponsored_transfer = Transaction::new_signed_with_payer(
        &[ix_sponsored_transfer],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_sponsored_transfer,
        "lamports_pda::sponsored_lamports_transfer",
    )
    .await
    .unwrap();

    let rent_pda_after = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must still exist");

    let lamports_pda_account = context
        .banks_client
        .get_account(lamports_pda)
        .await
        .unwrap()
        .expect("lamports pda must exist");
    assert_eq!(
        lamports_pda_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );
    assert_eq!(lamports_pda_account.data.len(), 0);
    assert_eq!(lamports_pda_account.lamports, sponsored_rent + TRANSFER_AMOUNT);
    let destination_account = context
        .banks_client
        .get_account(destination.pubkey())
        .await
        .unwrap()
        .expect("destination must exist");
    assert_eq!(destination_account.lamports, DESTINATION_STARTING_LAMPORTS);

    let delegation_record_account = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    let record_len = DelegationRecord::size_with_discriminator();
    let record = DelegationRecord::try_from_bytes_with_discriminator(&delegation_record_account.data[..record_len])
        .expect("delegation record must deserialize");
    assert_eq!(record.owner.to_bytes(), PROGRAM.to_bytes());
    assert_eq!(record.authority.to_bytes(), validator.to_bytes());
    assert!(
        !delegation_record_account.data[record_len..].is_empty(),
        "expected stored post-delegation payload bytes"
    );
    assert_eq!(
        rent_pda_after.lamports,
        rent_pda_before.lamports + SPONSORED_LAMPORTS_TRANSFER_SETUP_LAMPORTS
            - sponsored_rent
            - delegation_record_account.lamports
            - context
                .banks_client
                .get_account(delegation_metadata_pda)
                .await
                .unwrap()
                .expect("delegation metadata must exist")
                .lamports
    );
}

#[tokio::test]
async fn transfer_lamports_pda_moves_requested_lamports_to_destination() {
    let destination = utils::test_keypair("transfer_lamports_pda_moves_requested_lamports_to_destination::destination");

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            destination.pubkey(),
            Account {
                lamports: DESTINATION_STARTING_LAMPORTS,
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
    let sponsored_rent = context.banks_client.get_rent().await.unwrap().minimum_balance(0);
    let (lamports_pda, _) = derive_lamports_pda(PROGRAM, payer, destination.pubkey(), SALT);

    context.set_account(
        &lamports_pda,
        &Account {
            lamports: sponsored_rent + TRANSFER_AMOUNT,
            data: vec![],
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );

    let mut transfer_lamports_data = TestInternalInstruction::TransferLamportsPda.to_vec();
    transfer_lamports_data.extend_from_slice(&TRANSFER_AMOUNT.to_le_bytes());
    transfer_lamports_data.extend_from_slice(&SALT);

    let ix_transfer_lamports = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(lamports_pda, false),
            AccountMeta::new(destination.pubkey(), false),
        ],
        data: transfer_lamports_data,
    };
    let tx_transfer_lamports = Transaction::new_signed_with_payer(
        &[ix_transfer_lamports],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_transfer_lamports,
        "lamports_pda::transfer_lamports_pda",
    )
    .await
    .unwrap();

    let lamports_pda_account = context
        .banks_client
        .get_account(lamports_pda)
        .await
        .unwrap()
        .expect("lamports pda must exist");
    assert_eq!(lamports_pda_account.owner, PROGRAM);
    assert_eq!(lamports_pda_account.lamports, sponsored_rent);

    let destination_account = context
        .banks_client
        .get_account(destination.pubkey())
        .await
        .unwrap()
        .expect("destination must exist");
    assert_eq!(
        destination_account.lamports,
        DESTINATION_STARTING_LAMPORTS + TRANSFER_AMOUNT
    );
}

#[tokio::test]
async fn transfer_lamports_pda_allows_extra_lamports_on_source() {
    let destination = utils::test_keypair("transfer_lamports_pda_allows_extra_lamports_on_source::destination");
    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            destination.pubkey(),
            Account {
                lamports: DESTINATION_STARTING_LAMPORTS,
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
    let sponsored_rent = context.banks_client.get_rent().await.unwrap().minimum_balance(0);
    let (lamports_pda, _) = derive_lamports_pda(PROGRAM, payer, destination.pubkey(), SALT);

    context.set_account(
        &lamports_pda,
        &Account {
            lamports: sponsored_rent + TRANSFER_AMOUNT + DUST_LAMPORTS,
            data: vec![],
            owner: PROGRAM,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );

    let mut transfer_lamports_data = TestInternalInstruction::TransferLamportsPda.to_vec();
    transfer_lamports_data.extend_from_slice(&TRANSFER_AMOUNT.to_le_bytes());
    transfer_lamports_data.extend_from_slice(&SALT);

    let ix_transfer_lamports = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(lamports_pda, false),
            AccountMeta::new(destination.pubkey(), false),
        ],
        data: transfer_lamports_data,
    };
    let tx_transfer_lamports = Transaction::new_signed_with_payer(
        &[ix_transfer_lamports],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_transfer_lamports,
        "lamports_pda::allow_extra_lamports",
    )
    .await
    .unwrap();

    let lamports_pda_account = context
        .banks_client
        .get_account(lamports_pda)
        .await
        .unwrap()
        .expect("lamports pda must exist");
    assert_eq!(lamports_pda_account.owner, PROGRAM);
    assert_eq!(lamports_pda_account.lamports, sponsored_rent + DUST_LAMPORTS);

    let destination_account = context
        .banks_client
        .get_account(destination.pubkey())
        .await
        .unwrap()
        .expect("destination must exist");
    assert_eq!(
        destination_account.lamports,
        DESTINATION_STARTING_LAMPORTS + TRANSFER_AMOUNT
    );
}

fn close_lamports_pda_ix(payer: Pubkey, rent_pda: Pubkey, lamports_pda: Pubkey, destination: Pubkey) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, false),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new(lamports_pda, false),
            AccountMeta::new_readonly(destination, false),
        ],
        data: ESplInstruction::CloseLamportsPda.with_data(&SALT),
    }
}

async fn start_close_lamports_pda_test(
    label: &str,
    lamports_pda_owner: Pubkey,
) -> (ProgramTestContext, Pubkey, Pubkey, Pubkey, Pubkey, u64) {
    let payer = utils::test_pubkey(&format!("{label}::payer"));
    let destination = utils::test_pubkey(&format!("{label}::destination"));
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);
    let (lamports_pda, _) = derive_lamports_pda(PROGRAM, payer, destination, SALT);
    let system_account = |lamports| Account {
        lamports,
        data: vec![],
        owner: solana_system_interface::program::ID,
        executable: false,
        rent_epoch: 0,
    };
    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(payer, system_account(DESTINATION_STARTING_LAMPORTS));
        pt.add_account(rent_pda, system_account(RENT_PDA_STARTING_LAMPORTS));
    })
    .await;
    let sponsored_rent = context.banks_client.get_rent().await.unwrap().minimum_balance(0);
    context.set_account(
        &lamports_pda,
        &Account {
            lamports: sponsored_rent + TRANSFER_AMOUNT,
            data: vec![],
            owner: lamports_pda_owner,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );
    (context, payer, destination, rent_pda, lamports_pda, sponsored_rent)
}

#[tokio::test]
async fn close_lamports_pda_refunds_principal_to_payer_and_rent_to_rent_pda() {
    let (context, payer, destination, rent_pda, lamports_pda, sponsored_rent) =
        start_close_lamports_pda_test("close_lamports_pda_refunds", PROGRAM).await;

    let cranker_kp = utils::fixed_payer_keypair();
    let tx = Transaction::new_signed_with_payer(
        &[close_lamports_pda_ix(payer, rent_pda, lamports_pda, destination)],
        Some(&cranker_kp.pubkey()),
        &[&cranker_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "lamports_pda::close_lamports_pda")
        .await
        .unwrap();

    assert!(context.banks_client.get_account(lamports_pda).await.unwrap().is_none());
    let payer_account = context
        .banks_client
        .get_account(payer)
        .await
        .unwrap()
        .expect("payer must exist");
    assert_eq!(payer_account.lamports, DESTINATION_STARTING_LAMPORTS + TRANSFER_AMOUNT);
    let rent_pda_account = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must exist");
    assert_eq!(rent_pda_account.lamports, RENT_PDA_STARTING_LAMPORTS + sponsored_rent);
}

#[tokio::test]
async fn close_lamports_pda_rejects_delegated_pda() {
    let (context, payer, destination, rent_pda, lamports_pda, _) =
        start_close_lamports_pda_test("close_lamports_pda_rejects_delegated", ephemeral_rollups_pinocchio::ID).await;

    let cranker_kp = utils::fixed_payer_keypair();
    let tx = Transaction::new_signed_with_payer(
        &[close_lamports_pda_ix(payer, rent_pda, lamports_pda, destination)],
        Some(&cranker_kp.pubkey()),
        &[&cranker_kp],
        context.last_blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &context.banks_client,
        tx,
        "lamports_pda::close_lamports_pda_rejects_delegated",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidAccountOwner)
    );
    assert_eq!(
        context
            .banks_client
            .get_account(lamports_pda)
            .await
            .unwrap()
            .unwrap()
            .owner,
        ephemeral_rollups_pinocchio::ID
    );
}

#[tokio::test]
async fn close_lamports_pda_rejects_substituted_payer() {
    let (context, _payer, destination, rent_pda, lamports_pda, _) =
        start_close_lamports_pda_test("close_lamports_pda_rejects_substituted_payer", PROGRAM).await;

    let cranker_kp = utils::fixed_payer_keypair();
    let tx = Transaction::new_signed_with_payer(
        &[close_lamports_pda_ix(
            cranker_kp.pubkey(),
            rent_pda,
            lamports_pda,
            destination,
        )],
        Some(&cranker_kp.pubkey()),
        &[&cranker_kp],
        context.last_blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &context.banks_client,
        tx,
        "lamports_pda::close_lamports_pda_rejects_substituted_payer",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        TransactionError::InstructionError(0, InstructionError::InvalidSeeds)
    );
}
