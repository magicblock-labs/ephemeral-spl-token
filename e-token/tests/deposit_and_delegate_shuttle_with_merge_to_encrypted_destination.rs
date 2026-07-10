use dlp_api::state::DelegationRecord;
use ephemeral_rollups_pinocchio::pda::{
    delegate_buffer_pda_from_delegated_account_and_owner_program, delegation_metadata_pda_from_delegated_account,
    delegation_record_pda_from_delegated_account,
};
use ephemeral_spl_api::{
    consts::{
        SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS, SPONSORED_SHUTTLE_MERGE_TO_ENCRYPTED_DESTINATION_EXTRA_LAMPORTS,
    },
    instruction,
    instructions::DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs,
    state::{ephemeral_ata::EphemeralAta, load, shuttle_ephemeral_ata::ShuttleMetadata, Initializable},
    ID as PROGRAM,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::rent::Rent;
use solana_program_pack::Pack;
use solana_program_test::tokio;
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account as SplAccount;
use wheels::layout::Encodable as _;

mod common;
mod utils;

const RENT_PDA_SEED: &[u8] = b"rent";
const MAGIC_PROGRAM: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000 * 10u64.pow(DECIMALS as u32);
const DEPOSIT_AMOUNT: u64 = 100 * 10u64.pow(DECIMALS as u32);

fn encrypt_pubkey(pubkey: &Pubkey, validator: &Pubkey) -> [u8; 80] {
    dlp_api::encryption::encrypt_ed25519_recipient(pubkey.as_array(), &validator.to_bytes())
        .expect("validator key should be valid for encryption")
        .try_into()
        .expect("encrypted pubkey must be 80 bytes")
}

fn contains_window(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[tokio::test]
async fn deposit_and_delegate_shuttle_with_merge_to_encrypted_destination_stores_encrypted_action() {
    let owner = utils::test_keypair("merge_to_encrypted_destination::owner");
    let owner_token = utils::test_keypair("merge_to_encrypted_destination::owner_token");

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
    let mint_kp = utils::test_keypair("merge_to_encrypted_destination::mint");
    let mint = mint_kp.pubkey();
    let shuttle_id = 11_u32;
    let validator = utils::test_keypair("merge_to_encrypted_destination::validator").pubkey();

    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let _setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, STARTING_BALANCE, 1).await;

    let destination_owner = utils::test_pubkey("merge_to_encrypted_destination::destination_owner");
    let destination_ata = utils::derive_associated_token_address(destination_owner, mint);

    let (shuttle_metadata, _) = ShuttleMetadata::find_pda(&owner.pubkey(), &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle_metadata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);
    let pdas = utils::derive_pdas(PROGRAM, owner.pubkey(), mint);
    let vault = pdas.vault;
    let (vault_eata, _) = EphemeralAta::find_pda(&vault, &mint);
    let vault_ata = utils::derive_associated_token_address(vault, mint);
    let owner_source_ata = owner_token.pubkey();

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
            ix_create_owner_source,
            ix_init_owner_source,
            ix_mint_owner_source,
        ],
        Some(&payer),
        &[&payer_kp, &owner_token],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx_init).await.unwrap();

    let rent_pda_before = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must exist");

    let buffer_pda = delegate_buffer_pda_from_delegated_account_and_owner_program(&shuttle_eata, &PROGRAM);
    let delegation_record_pda = delegation_record_pda_from_delegated_account(&shuttle_eata);
    let delegation_metadata_pda = delegation_metadata_pda_from_delegated_account(&shuttle_eata);

    let encrypted_destination_owner = encrypt_pubkey(&destination_owner, &validator);
    let encrypted_destination_ata = encrypt_pubkey(&destination_ata, &validator);

    let args = DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs {
        shuttle_id,
        amount: DEPOSIT_AMOUNT,
        encrypted_destination_owner,
        encrypted_destination_ata,
        validator: Some(validator),
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
        ],
        data: instruction::ESplInstruction::DepositAndDelegateShuttleWithMergeToEncryptedDestination
            .with_data(&args.encode().unwrap()),
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp, &owner],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        "merge_to_encrypted_destination::shuttle",
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
    // No protocol fee on the base->ephemeral route.
    assert_eq!(shuttle_eata_state.amount, DEPOSIT_AMOUNT);

    let owner_source_account = context
        .banks_client
        .get_account(owner_source_ata)
        .await
        .unwrap()
        .expect("owner source token account must exist");
    let owner_source_state = SplAccount::unpack(&owner_source_account.data).unwrap();
    assert_eq!(owner_source_state.amount, STARTING_BALANCE - DEPOSIT_AMOUNT);

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
        delegation_record_account.data.len() > record_len,
        "expected stored post-delegation payload bytes"
    );
    let action_payload = &delegation_record_account.data[record_len..];

    // The destination keys must be carried only as ciphertexts.
    for (ciphertext, plaintext, label) in [
        (
            encrypted_destination_owner.as_slice(),
            destination_owner.to_bytes(),
            "destination owner",
        ),
        (
            encrypted_destination_ata.as_slice(),
            destination_ata.to_bytes(),
            "destination ata",
        ),
    ] {
        assert!(
            contains_window(action_payload, ciphertext),
            "expected stored post-delegation payload to embed the encrypted {label}"
        );
        assert!(
            !contains_window(action_payload, &plaintext),
            "stored post-delegation payload must not leak the plaintext {label}"
        );
    }

    // Cleartext accounts required by the ER-side instruction 33.
    for (key, label) in [
        (shuttle_wallet_ata.to_bytes(), "shuttle wallet ata"),
        (MAGIC_PROGRAM.to_bytes(), "magic program"),
    ] {
        assert!(
            contains_window(action_payload, &key),
            "expected stored post-delegation payload to reference the {label}"
        );
    }

    let merge_prefix = instruction::ESplInstruction::MergeShuttleIntoRentPendingAta.to_vec();
    assert!(
        contains_window(action_payload, &merge_prefix),
        "expected stored post-delegation payload to carry the instruction 33 discriminator"
    );

    let rent_pda_after = context
        .banks_client
        .get_account(rent_pda)
        .await
        .unwrap()
        .expect("rent pda must still exist");
    let delegation_metadata_account = context
        .banks_client
        .get_account(delegation_metadata_pda)
        .await
        .unwrap()
        .expect("delegation metadata must exist");
    assert_eq!(
        rent_pda_after.lamports,
        rent_pda_before.lamports
            + SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS
            + SPONSORED_SHUTTLE_MERGE_TO_ENCRYPTED_DESTINATION_EXTRA_LAMPORTS
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

#[tokio::test]
async fn deposit_and_delegate_shuttle_with_merge_to_encrypted_destination_rejects_zero_amount() {
    let owner = utils::test_keypair("merge_to_encrypted_destination_zero::owner");

    let context = utils::start_program_test_with(PROGRAM, |pt| {
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
    let mint = utils::test_pubkey("merge_to_encrypted_destination_zero::mint");
    let shuttle_id = 12_u32;
    let validator = utils::test_keypair("merge_to_encrypted_destination_zero::validator").pubkey();

    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);
    let destination_owner = utils::test_pubkey("merge_to_encrypted_destination_zero::destination_owner");
    let destination_ata = utils::derive_associated_token_address(destination_owner, mint);

    let (shuttle_metadata, _) = ShuttleMetadata::find_pda(&owner.pubkey(), &mint, shuttle_id);
    let (shuttle_eata, _) = EphemeralAta::find_pda(&shuttle_metadata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);
    let pdas = utils::derive_pdas(PROGRAM, owner.pubkey(), mint);
    let vault = pdas.vault;
    let vault_ata = utils::derive_associated_token_address(vault, mint);
    let owner_source_ata = utils::test_pubkey("merge_to_encrypted_destination_zero::owner_source");

    let args = DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs {
        shuttle_id,
        amount: 0,
        encrypted_destination_owner: encrypt_pubkey(&destination_owner, &validator),
        encrypted_destination_ata: encrypt_pubkey(&destination_ata, &validator),
        validator: Some(validator),
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
            AccountMeta::new(
                delegate_buffer_pda_from_delegated_account_and_owner_program(&shuttle_eata, &PROGRAM),
                false,
            ),
            AccountMeta::new(delegation_record_pda_from_delegated_account(&shuttle_eata), false),
            AccountMeta::new(delegation_metadata_pda_from_delegated_account(&shuttle_eata), false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(owner_source_ata, false),
            AccountMeta::new(vault_ata, false),
        ],
        data: instruction::ESplInstruction::DepositAndDelegateShuttleWithMergeToEncryptedDestination
            .with_data(&args.encode().unwrap()),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp, &owner],
        context.last_blockhash,
    );
    let error = context
        .banks_client
        .process_transaction(tx)
        .await
        .expect_err("zero amount must be rejected");
    assert_eq!(
        error.unwrap(),
        solana_transaction::TransactionError::InstructionError(
            0,
            solana_transaction::InstructionError::InvalidInstructionData
        )
    );
}
