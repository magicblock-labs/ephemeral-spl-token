use dlp::pda::{fees_vault_pda, validator_fees_vault_pda_from_validator};
use ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID;
use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
use ephemeral_spl_api::program::ID;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::bpf_loader;
use solana_program::example_mocks::solana_sdk::system_program;
use solana_program::native_token::LAMPORTS_PER_SOL;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn undelegate_ephemeral_ata_permission_callback() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    let payer = Keypair::new();
    let payer_pubkey = payer.pubkey();
    let mint = Keypair::new().pubkey();

    let (ephemeral_ata, _bump) =
        Pubkey::find_program_address(&[payer_pubkey.as_ref(), mint.as_ref()], &PROGRAM);
    let (permission_pda, _perm_bump) = Pubkey::find_program_address(
        &[b"permission:", ephemeral_ata.as_ref()],
        &PERMISSION_PROGRAM_ID.into(),
    );

    let data = read_file("tests/fixtures/dlp.so");
    pt.add_account(
        ephemeral_rollups_pinocchio::ID.into(),
        Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        payer_pubkey,
        Account {
            lamports: LAMPORTS_PER_SOL,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        permission_pda,
        Account {
            lamports: LAMPORTS_PER_SOL,
            data: vec![],
            owner: ephemeral_rollups_pinocchio::ID.into(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let seeds: [&[u8]; 2] = [b"permission:", ephemeral_ata.as_ref()];

    let mut delegation_record_data =
        vec![0u8; dlp::state::DelegationRecord::size_with_discriminator()];
    let delegation_record = dlp::state::DelegationRecord {
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
            &DELEGATION_PROGRAM_ID.into(),
        )
        .0,
        Account {
            lamports: Rent::default().minimum_balance(delegation_record_data.len()),
            data: delegation_record_data,
            owner: DELEGATION_PROGRAM_ID.into(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let delegation_metadata = dlp::state::DelegationMetadata {
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
            &DELEGATION_PROGRAM_ID.into(),
        )
        .0,
        Account {
            lamports: Rent::default().minimum_balance(delegation_metadata_data.len()),
            data: delegation_metadata_data,
            owner: DELEGATION_PROGRAM_ID.into(),
            executable: false,
            rent_epoch: 0,
        },
    );

    pt.add_account(
        fees_vault_pda().to_bytes().into(),
        Account {
            lamports: Rent::default().minimum_balance(0),
            data: vec![],
            owner: DELEGATION_PROGRAM_ID.into(),
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
            owner: DELEGATION_PROGRAM_ID.into(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let context = pt.start_with_context().await;

    let ix_undelegate = dlp::instruction_builder::undelegate(
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

    context.banks_client.process_transaction(tx).await.unwrap();

    let delegation_pda = Pubkey::find_program_address(
        &[b"delegation", permission_pda.to_bytes().as_slice()],
        &DELEGATION_PROGRAM_ID.into(),
    )
    .0;
    let delegation_metadata_pda = Pubkey::find_program_address(
        &[b"delegation-metadata", permission_pda.to_bytes().as_slice()],
        &DELEGATION_PROGRAM_ID.into(),
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
        delegation_account.is_none()
            || delegation_account.unwrap().owner != DELEGATION_PROGRAM_ID.into()
    );

    if let Some(account) = delegation_metadata_account {
        let metadata =
            dlp::state::DelegationMetadata::try_from_bytes_with_discriminator(&account.data)
                .unwrap();
        assert!(!metadata.is_undelegatable);
    }
}
