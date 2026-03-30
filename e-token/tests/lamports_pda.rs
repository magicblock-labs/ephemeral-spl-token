use dlp_api::state::DelegationRecord;
use ephemeral_spl_api::{
    consts::SPONSORED_LAMPORTS_TRANSFER_SETUP_LAMPORTS,
    instruction::{self, internal},
    program::ID,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{bpf_loader, rent::Rent};
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
const RENT_PDA_SEED: &[u8] = b"rent";
const LAMPORTS_PDA_SEED: &[u8] = b"lamports";
const DESTINATION_STARTING_LAMPORTS: u64 = 7;
const TRANSFER_AMOUNT: u64 = 2_500_000;
const SALT: [u8; 32] = [42; 32];
const DUST_LAMPORTS: u64 = 11;

fn derive_lamports_pda(
    program: Pubkey,
    payer: Pubkey,
    destination: Pubkey,
    salt: [u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            LAMPORTS_PDA_SEED,
            payer.as_ref(),
            destination.as_ref(),
            salt.as_ref(),
        ],
        &program,
    )
}

#[tokio::test]
async fn sponsored_lamports_transfer_delegates_zero_data_pda_and_charges_fee() {
    let destination = Keypair::new();

    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let data = read_file("tests/fixtures/dlp.so");
    let validator = Pubkey::new_unique();
    let (destination_delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", destination.pubkey().as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let mut destination_delegation_record_data =
        vec![0u8; DelegationRecord::size_with_discriminator()];
    DelegationRecord {
        authority: validator.to_bytes().into(),
        owner: solana_system_interface::program::ID.to_bytes().into(),
        delegation_slot: 0,
        lamports: DESTINATION_STARTING_LAMPORTS,
        commit_frequency_ms: 0,
    }
    .to_bytes_with_discriminator(&mut destination_delegation_record_data)
    .unwrap();
    pt.add_account(
        ephemeral_rollups_pinocchio::ID,
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );
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

    let context = &mut pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let rent = context.banks_client.get_rent().await.unwrap();
    let sponsored_rent = rent.minimum_balance(0);

    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);
    let (lamports_pda, _) = derive_lamports_pda(PROGRAM, payer, destination.pubkey(), SALT);
    let (buffer_pda, _) =
        Pubkey::find_program_address(&[b"buffer", lamports_pda.as_ref()], &PROGRAM);
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", lamports_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", lamports_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_init_rent = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_RENT_PDA],
    };
    let ix_fund_rent = transfer(&payer, &rent_pda, 100_000_000);
    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_rent, ix_fund_rent],
        Some(&payer),
        &[&context.payer],
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

    let mut sponsored_transfer_data = vec![instruction::SPONSORED_LAMPORTS_TRANSFER];
    sponsored_transfer_data.extend_from_slice(&TRANSFER_AMOUNT.to_le_bytes());
    sponsored_transfer_data.extend_from_slice(&SALT);

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
        data: sponsored_transfer_data,
    };
    let tx_sponsored_transfer = Transaction::new_signed_with_payer(
        &[ix_sponsored_transfer],
        Some(&payer),
        &[&context.payer],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(tx_sponsored_transfer)
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
    assert_eq!(
        lamports_pda_account.lamports,
        sponsored_rent + TRANSFER_AMOUNT
    );
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
    let record = DelegationRecord::try_from_bytes_with_discriminator(
        &delegation_record_account.data[..record_len],
    )
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
    let destination = Keypair::new();

    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
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

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let sponsored_rent = context
        .banks_client
        .get_rent()
        .await
        .unwrap()
        .minimum_balance(0);
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

    let mut transfer_lamports_data = vec![internal::TRANSFER_LAMPORTS_PDA];
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
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_transfer_lamports)
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
    let destination = Keypair::new();

    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
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

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let sponsored_rent = context
        .banks_client
        .get_rent()
        .await
        .unwrap()
        .minimum_balance(0);
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

    let mut transfer_lamports_data = vec![internal::TRANSFER_LAMPORTS_PDA];
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
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_transfer_lamports)
        .await
        .unwrap();

    let lamports_pda_account = context
        .banks_client
        .get_account(lamports_pda)
        .await
        .unwrap()
        .expect("lamports pda must exist");
    assert_eq!(lamports_pda_account.owner, PROGRAM);
    assert_eq!(
        lamports_pda_account.lamports,
        sponsored_rent + DUST_LAMPORTS
    );

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
