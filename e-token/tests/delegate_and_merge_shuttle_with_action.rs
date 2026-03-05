use borsh::BorshDeserialize;
use dlp_api::dlp::state::DelegationRecord;
use dlp_api::encryption::encrypt_ed25519_recipient;
use ephemeral_rollups_pinocchio::instruction::{
    AccountMeta as CompactAccountMeta, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction,
    MaybeEncryptedIxData, MaybeEncryptedPubkey, PostDelegationActions,
};
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::RawType;
use ephemeral_spl_api::{args::DelegateShuttleWithActionArgs, instruction};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::bpf_loader;
use solana_program::rent::Rent;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn delegate_and_merge_shuttle_with_action_succeeds() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(true);
    utils::add_associated_token_program(&mut pt);

    let data = read_file("tests/fixtures/dlp.so");
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

    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let owner = payer; // the owner is the payer
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();
    let shuttle_id = 9_u32;

    let setup =
        utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;
    let owner_ata = setup.user_tokens[0];

    let (shuttle_ephemeral_ata, shuttle_bump) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner, mint, shuttle_id);
    let (shuttle_eata, shuttle_eata_bump) =
        utils::derive_shuttle_eata(PROGRAM, shuttle_ephemeral_ata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_ephemeral_ata, mint);

    let mut init_data = vec![instruction::INITIALIZE_SHUTTLE_EPHEMERAL_ATA];
    init_data.extend_from_slice(&shuttle_id.to_le_bytes());
    init_data.push(shuttle_bump);

    let ix_init_shuttle = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: init_data,
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_shuttle],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let shuttle_meta_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(
        shuttle_meta_account.data.len(),
        ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta::LEN
    );
    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata account must exist");
    assert_eq!(shuttle_eata_account.data.len(), EphemeralAta::LEN);

    let (buffer_pda, _) = Pubkey::find_program_address(
        &[b"buffer", shuttle_eata.as_ref()],
        &ephemeral_spl_api::program::id().into(),
    );
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", shuttle_eata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", shuttle_eata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let program_id = Pubkey::new_unique();
    let acc1 = Pubkey::new_unique();
    let acc2 = Pubkey::new_unique();

    let encrypted_accounts = vec![
        encrypt_ed25519_recipient(program_id.as_array(), payer.as_array())
            .unwrap()
            .into(),
        encrypt_ed25519_recipient(acc1.as_array(), payer.as_array())
            .unwrap()
            .into(),
        encrypt_ed25519_recipient(acc2.as_array(), payer.as_array())
            .unwrap()
            .into(),
    ];
    let encrypted_accounts_meta = vec![
        encrypt_ed25519_recipient(
            &[CompactAccountMeta::new(11, true).to_byte()],
            payer.as_array(),
        )
        .unwrap()
        .into(),
        encrypt_ed25519_recipient(
            &[CompactAccountMeta::new(12, false).to_byte()],
            payer.as_array(),
        )
        .unwrap()
        .into(),
    ];
    let encrypted_ix_data = MaybeEncryptedIxData {
        prefix: vec![],
        suffix: encrypt_ed25519_recipient(b"test data", payer.as_array())
            .unwrap()
            .into(),
    };
    let ix_args = DelegateShuttleWithActionArgs {
        bump: shuttle_eata_bump,
        validator: None,
        accounts: encrypted_accounts.clone(),
        ix: MaybeEncryptedInstruction {
            program_id: 10,
            accounts: encrypted_accounts_meta.clone(),
            data: encrypted_ix_data.clone(),
        },
    };

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(owner_ata, false),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new_readonly(shuttle_wallet_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: [
            vec![instruction::DELEGATE_AND_MERGE_SHUTTLE_WITH_ACTION],
            borsh::to_vec(&ix_args).unwrap(),
        ]
        .concat(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let record = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    assert!(
        record.data.len() > DelegationRecord::size_with_discriminator(),
        "delegation record must be at least {} bytes, actual: {}",
        DelegationRecord::size_with_discriminator(),
        record.data.len()
    );

    let action = PostDelegationActions::try_from_slice(
        &record.data[DelegationRecord::size_with_discriminator()..],
    )
    .unwrap();
    assert_eq!(action.instructions.len(), 3);
    assert_eq!(action.instructions[0].program_id, 9);
    assert_eq!(action.instructions[1].program_id, 10);
    assert_eq!(action.instructions[2].program_id, 9);

    for i in 0..3 {
        let MaybeEncryptedPubkey::Encrypted(encrypted_account) = &action.non_signers[10 + i] else {
            panic!("encrypted account must be encrypted");
        };
        let MaybeEncryptedPubkey::Encrypted(original_account) = &encrypted_accounts[i] else {
            panic!("encrypted account must be encrypted");
        };
        assert_eq!(encrypted_account.as_bytes(), original_account.as_bytes());
    }

    for i in 0..2 {
        let MaybeEncryptedAccountMeta::Encrypted(encrypted_account_meta) =
            &action.instructions[1].accounts[i]
        else {
            panic!("encrypted account meta must be encrypted");
        };
        let MaybeEncryptedAccountMeta::Encrypted(original_account_meta) =
            &encrypted_accounts_meta[i]
        else {
            panic!("encrypted account meta must be encrypted");
        };
        assert_eq!(
            encrypted_account_meta.as_bytes(),
            original_account_meta.as_bytes()
        );
    }

    assert_eq!(action.instructions[1].data.prefix, encrypted_ix_data.prefix);
    assert_eq!(
        action.instructions[1].data.suffix.as_bytes(),
        encrypted_ix_data.suffix.as_bytes()
    );

    let shuttle_meta_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");
    assert_eq!(shuttle_meta_account.owner, PROGRAM);

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
}
