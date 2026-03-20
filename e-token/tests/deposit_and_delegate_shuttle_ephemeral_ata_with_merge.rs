use dlp_api::state::DelegationRecord;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{bpf_loader, rent::Rent};
use solana_program_pack::Pack;
use solana_program_test::{read_file, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account as SplAccount;

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
const RENT_PDA_SEED: &[u8] = b"rent";
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000 * 10u64.pow(DECIMALS as u32);
const DEPOSIT_AMOUNT: u64 = 100 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn deposit_and_delegate_shuttle_ephemeral_ata_with_merge_deposits_and_stores_post_delegation_action(
) {
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
    let owner_token = Keypair::new();
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
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;
    let destination_ata = setup.user_tokens[0];

    let (shuttle_metadata, _) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner.pubkey(), mint, shuttle_id);
    let (shuttle_eata, _) = utils::derive_shuttle_eata(PROGRAM, shuttle_metadata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);
    let pdas = utils::derive_pdas(PROGRAM, owner.pubkey(), mint);
    let vault = pdas.vault;
    let (vault_eata, _) = Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
    let vault_ata = utils::derive_associated_token_address(vault, mint);
    let owner_source_ata = owner_token.pubkey();

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
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
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
        &[&context.payer, &owner_token],
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

    let mut delegate_data = vec![instruction::SETUP_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE];
    delegate_data.extend_from_slice(&shuttle_id.to_le_bytes());
    delegate_data.extend_from_slice(&DEPOSIT_AMOUNT.to_le_bytes());
    delegate_data.extend_from_slice(&validator.to_bytes());

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
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(owner_source_ata, false),
            AccountMeta::new(vault_ata, false),
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

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_metadata)
        .await
        .unwrap()
        .expect("shuttle metadata must exist");
    assert_eq!(shuttle_account.owner, PROGRAM);
    let mut shuttle_data = shuttle_account.data.clone();
    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleMetadata>(shuttle_data.as_mut_slice()).unwrap() };
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
    let shuttle_eata_state =
        unsafe { load_mut_unchecked::<EphemeralAta>(shuttle_eata_data.as_mut_slice()).unwrap() };
    assert_eq!(shuttle_eata_state.amount, DEPOSIT_AMOUNT);

    let shuttle_wallet_account = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_state = SplAccount::unpack(&shuttle_wallet_account.data).unwrap();
    assert_eq!(shuttle_wallet_state.owner, shuttle_metadata);
    assert_eq!(shuttle_wallet_state.mint, mint);

    let owner_source_account = context
        .banks_client
        .get_account(owner_source_ata)
        .await
        .unwrap()
        .expect("owner source token account must exist");
    let owner_source_state = SplAccount::unpack(&owner_source_account.data).unwrap();
    assert_eq!(owner_source_state.owner, owner.pubkey());
    assert_eq!(owner_source_state.amount, STARTING_BALANCE - DEPOSIT_AMOUNT);

    let vault_account = context
        .banks_client
        .get_account(vault_ata)
        .await
        .unwrap()
        .expect("vault token account must exist");
    let vault_state = SplAccount::unpack(&vault_account.data).unwrap();
    assert_eq!(vault_state.amount, DEPOSIT_AMOUNT);

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
        "post-delegation payload should be stored for the merge and cleanup actions"
    );
}
