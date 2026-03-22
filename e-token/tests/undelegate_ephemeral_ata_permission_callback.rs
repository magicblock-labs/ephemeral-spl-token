use dlp_api::pda::{fees_vault_pda, validator_fees_vault_pda_from_validator};
use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
use ephemeral_spl_api::program::ID;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::native_token::LAMPORTS_PER_SOL;
use solana_program::rent::Rent;
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn undelegate_ephemeral_ata_permission_callback() {
    let permission_program_id = utils::permission_program_id();

    let payer = utils::fixed_payer_keypair();
    let payer_pubkey = payer.pubkey();
    let mint = utils::test_keypair("undelegate_ephemeral_ata_permission_callback::mint").pubkey();

    let (ephemeral_ata, _) =
        Pubkey::find_program_address(&[payer_pubkey.as_ref(), mint.as_ref()], &PROGRAM);
    let (permission_pda, _) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &permission_program_id,
    );

    let context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            permission_pda,
            Account {
                lamports: LAMPORTS_PER_SOL,
                data: vec![],
                owner: ephemeral_rollups_pinocchio::ID,
                executable: false,
                rent_epoch: 0,
            },
        );

        let seeds: [&[u8]; 2] = [b"permission:", ephemeral_ata.as_ref()];

        let mut delegation_record_data =
            vec![0u8; dlp_api::state::DelegationRecord::size_with_discriminator()];
        let delegation_record = dlp_api::state::DelegationRecord {
            authority: payer_pubkey.to_bytes().into(),
            owner: PROGRAM.to_bytes().into(),
            delegation_slot: 0,
            commit_frequency_ms: 0,
            lamports: Rent::default().minimum_balance(delegation_record_data.len()),
        };
        delegation_record
            .to_bytes_with_discriminator(&mut delegation_record_data)
            .unwrap();
        pt.add_account(
            Pubkey::find_program_address(
                &[b"delegation", permission_pda.to_bytes().as_slice()],
                &DELEGATION_PROGRAM_ID,
            )
            .0,
            Account {
                lamports: Rent::default().minimum_balance(delegation_record_data.len()),
                data: delegation_record_data,
                owner: DELEGATION_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        );

        let delegation_metadata = dlp_api::state::DelegationMetadata {
            last_update_nonce: 0,
            is_undelegatable: true,
            seeds: seeds.iter().map(|s| s.to_vec()).collect(),
            rent_payer: payer_pubkey.to_bytes().into(),
        };
        let mut delegation_metadata_data = vec![];
        delegation_metadata
            .to_bytes_with_discriminator(&mut delegation_metadata_data)
            .unwrap();
        pt.add_account(
            Pubkey::find_program_address(
                &[b"delegation-metadata", permission_pda.to_bytes().as_slice()],
                &DELEGATION_PROGRAM_ID,
            )
            .0,
            Account {
                lamports: Rent::default().minimum_balance(delegation_metadata_data.len()),
                data: delegation_metadata_data,
                owner: DELEGATION_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        );

        pt.add_account(
            fees_vault_pda().to_bytes().into(),
            Account {
                lamports: Rent::default().minimum_balance(0),
                data: vec![],
                owner: DELEGATION_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        );

        pt.add_account(
            validator_fees_vault_pda_from_validator(&payer_pubkey.to_bytes().into())
                .to_bytes()
                .into(),
            Account {
                lamports: LAMPORTS_PER_SOL,
                data: vec![],
                owner: DELEGATION_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let ix_undelegate = dlp_api::instruction_builder::undelegate(
        payer_pubkey.to_bytes().into(),
        permission_pda.to_bytes().into(),
        PROGRAM.to_bytes().into(),
        payer_pubkey.to_bytes().into(),
    );

    let ix_undelegate = Instruction::new_with_bytes(
        ix_undelegate.program_id.to_bytes().into(),
        ix_undelegate.data.as_slice(),
        ix_undelegate
            .accounts
            .iter()
            .map(|a| AccountMeta {
                pubkey: a.pubkey.to_bytes().into(),
                is_signer: a.is_signer,
                is_writable: a.is_writable,
            })
            .collect(),
    );

    let tx = Transaction::new_signed_with_payer(
        &[ix_undelegate],
        Some(&Pubkey::new_from_array(payer_pubkey.to_bytes())),
        &[&payer],
        context.last_blockhash,
    );

    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "undel_perm_cb::undelegate",
    )
    .await
    .unwrap();

    let delegation_pda = Pubkey::find_program_address(
        &[b"delegation", permission_pda.to_bytes().as_slice()],
        &DELEGATION_PROGRAM_ID,
    )
    .0;
    let delegation_metadata_pda = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.to_bytes().as_slice()],
        &DELEGATION_PROGRAM_ID,
    )
    .0;

    let permission_account = context
        .banks_client
        .get_account(permission_pda)
        .await
        .unwrap()
        .expect("permission account must exist");
    let delegation_account = context
        .banks_client
        .get_account(delegation_pda)
        .await
        .unwrap();
    let delegation_metadata_account = context
        .banks_client
        .get_account(delegation_metadata_pda)
        .await
        .unwrap();

    assert_eq!(permission_account.owner, PROGRAM);
    assert!(
        delegation_account.is_none() || delegation_account.unwrap().owner != DELEGATION_PROGRAM_ID
    );

    if let Some(account) = delegation_metadata_account {
        let metadata =
            dlp_api::state::DelegationMetadata::try_from_bytes_with_discriminator(&account.data)
                .unwrap();
        assert!(!metadata.is_undelegatable);
    }
}
