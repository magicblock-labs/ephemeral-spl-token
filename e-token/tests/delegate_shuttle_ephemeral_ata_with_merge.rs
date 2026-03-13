use dlp::state::DelegationRecord;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{bpf_loader, rent::Rent};
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

#[tokio::test]
async fn delegate_shuttle_ephemeral_ata_with_merge_stores_post_delegation_action() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    pt.prefer_bpf(true);

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

    let owner = Keypair::new();
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

    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();
    let shuttle_id = 9_u32;
    let validator = Pubkey::new_unique();

    let setup =
        utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;
    let destination_ata = setup.user_tokens[0];

    let (shuttle_ephemeral_ata, shuttle_bump) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner.pubkey(), mint, shuttle_id);
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
            AccountMeta::new_readonly(owner.pubkey(), false),
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

    let mut delegate_data = vec![
        instruction::DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE,
        shuttle_eata_bump,
    ];
    delegate_data.extend_from_slice(&validator.to_bytes());

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(owner.pubkey(), true),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: delegate_data,
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&context.payer, &owner],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(tx_delegate)
        .await
        .unwrap();

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
        "post-delegation payload should be stored for the merge action"
    );
}
