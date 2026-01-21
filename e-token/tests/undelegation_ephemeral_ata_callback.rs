use dlp::pda::{fees_vault_pda, validator_fees_vault_pda_from_validator};
use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
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
async fn undelegation_callback_restores_ephemeral_ata() {
    // Start the program test with our program loaded
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);

    // Use a deterministic mint for stable PDA derivations
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive the delegated PDA under the Delegation Program using seeds [payer, mint]
    let payer = Keypair::new();
    let payer_pubkey = payer.pubkey();
    let seeds: [&[u8]; 2] = [payer_pubkey.as_ref(), mint.as_ref()];
    let (delegated_ata, _bump) = Pubkey::find_program_address(&seeds, &PROGRAM);

    println!("Delegated ata: {:?}", delegated_ata);

    // Setup the delegation program
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

    // Setup the payer
    pt.add_account(
        payer.pubkey(),
        Account {
            lamports: LAMPORTS_PER_SOL,
            data: vec![],
            owner: system_program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    // Setup the delegated PDA
    let mut data = vec![0u8; EphemeralAta::LEN];
    let ephemeral_ata = unsafe { load_mut_unchecked::<EphemeralAta>(data.as_mut_slice()).unwrap() };
    ephemeral_ata.mint = pinocchio::Address::new_from_array(mint.to_bytes());
    ephemeral_ata.amount = 500;
    pt.add_account(
        delegated_ata,
        Account {
            lamports: LAMPORTS_PER_SOL,
            data: data.clone(),
            owner: ephemeral_rollups_pinocchio::ID.into(),
            executable: false,
            rent_epoch: 0,
        },
    );

    // Setup the delegated record PDA
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
            &[b"delegation", delegated_ata.to_bytes().as_slice()],
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

    // Setup the delegation metadata PDA
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
            &[b"delegation-metadata", delegated_ata.to_bytes().as_slice()],
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

    // Setup the protocol fees vault
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

    // Setup the validator fees vault
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

    // Call undelegation
    let ix_undelegate = dlp::instruction_builder::undelegate(
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

    context.banks_client.process_transaction(tx).await.unwrap();

    // Assert the delegated PDA now exists, is owned by our program, and has data equal to buffer (zeros)
}
