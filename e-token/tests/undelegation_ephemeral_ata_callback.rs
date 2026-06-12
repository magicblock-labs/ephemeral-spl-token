use dlp_api::pda::{fees_vault_pda, validator_fees_vault_pda_from_validator};
use ephemeral_rollups_pinocchio::{
    consts::DELEGATION_PROGRAM_ID,
    pda::{
        delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account,
    },
};
use ephemeral_spl_api::{
    state::{ephemeral_ata::EphemeralAta, load_mut, RawType},
    ID as PROGRAM,
};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{native_token::LAMPORTS_PER_SOL, rent::Rent};
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

#[tokio::test]
async fn undelegation_callback_restores_ephemeral_ata() {
    let payer = utils::fixed_payer_keypair();
    let payer_pubkey = payer.pubkey();

    let mint_kp = utils::test_keypair("undelegation_ephemeral_ata_callback::mint");
    let mint = mint_kp.pubkey();

    let seeds: [&[u8]; 2] = [payer_pubkey.as_ref(), mint.as_ref()];
    let (delegated_ata, _) = EphemeralAta::find_pda(&payer_pubkey, &mint);

    let context = utils::start_program_test_with(PROGRAM, |pt| {
        let mut data = vec![0u8; EphemeralAta::LEN];
        let ephemeral_ata = load_mut::<EphemeralAta>(data.as_mut_slice()).unwrap();
        ephemeral_ata.mint = Address::new_from_array(mint.to_bytes());
        ephemeral_ata.amount = 500;
        pt.add_account(
            delegated_ata,
            Account {
                lamports: LAMPORTS_PER_SOL,
                data: data.clone(),
                owner: ephemeral_rollups_pinocchio::ID,
                executable: false,
                rent_epoch: 0,
            },
        );

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
            delegation_record_pda_from_delegated_account(&delegated_ata),
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
            delegation_metadata_pda_from_delegated_account(&delegated_ata),
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
        delegated_ata.to_bytes().into(),
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
        "undel_eata_cb::undelegate",
    )
    .await
    .unwrap();

    // Assert the delegated PDA now exists, is owned by our program, and has data equal to buffer (zeros)
}
