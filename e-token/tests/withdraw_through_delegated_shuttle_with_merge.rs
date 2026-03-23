use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use dlp_api::state::DelegationRecord;
use ephemeral_spl_api::instruction::{self, internal};
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use ephemeral_spl_api::ID as PROGRAM;
use magicblock_magic_program_api::{
    args::{MagicIntentBundleArgs, UndelegateTypeArgs},
    instruction::MagicBlockInstruction,
    Pubkey as MagicPubkey, MAGIC_CONTEXT_PUBKEY,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError, rent::Rent,
};
use solana_program_pack::Pack;
use solana_program_test::{processor, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account as SplAccount;

mod common;
mod utils;

const RENT_PDA_SEED: &[u8] = b"rent";
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 1_000 * 10u64.pow(DECIMALS as u32);
const TRANSFER_AMOUNT: u64 = 100 * 10u64.pow(DECIMALS as u32);

#[derive(Clone)]
struct CapturedIntentBundle {
    schedule_accounts: Vec<Pubkey>,
    args: MagicIntentBundleArgs,
}

fn captured_intent_bundles() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clear_captured_intent_bundles(magic_program: Pubkey) {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .remove(&magic_program);
}

fn peek_captured_intent_bundles(magic_program: Pubkey) -> Vec<CapturedIntentBundle> {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .get(&magic_program)
        .cloned()
        .unwrap_or_default()
}

fn convert_magic_pubkey(pubkey: MagicPubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}

fn process_magic_program_mock(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let magic_ix: MagicBlockInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match magic_ix {
        MagicBlockInstruction::ScheduleIntentBundle(args) => {
            let Some(payer) = accounts.first() else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };
            if !payer.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }

            captured_intent_bundles()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedIntentBundle {
                    schedule_accounts: accounts.iter().map(|account| *account.key).collect(),
                    args,
                });
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }

    Ok(())
}

fn add_magic_program_mock(pt: &mut ProgramTest, magic_program: Pubkey) {
    pt.prefer_bpf(false);
    pt.add_program(
        "magic_program_mock",
        magic_program,
        processor!(process_magic_program_mock),
    );
    pt.prefer_bpf(true);
}

fn associated_token_create_idempotent_ix(
    funding: Pubkey,
    ata: Pubkey,
    wallet: Pubkey,
    mint: Pubkey,
) -> Instruction {
    Instruction {
        program_id: utils::associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(funding, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: vec![1],
    }
}

#[tokio::test]
async fn withdraw_through_delegated_shuttle_with_merge_stores_transfer_and_cleanup_actions() {
    let owner = utils::test_keypair(
        "withdraw_through_delegated_shuttle_with_merge_stores_transfer_and_cleanup_actions::owner",
    );
    let owner_token = utils::test_keypair(
        "withdraw_through_delegated_shuttle_with_merge_stores_transfer_and_cleanup_actions::owner_token",
    );

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
    let mint_kp = utils::test_keypair(
        "withdraw_through_delegated_shuttle_with_merge_stores_transfer_and_cleanup_actions::mint",
    );
    let mint = mint_kp.pubkey();
    let shuttle_id = 9_u32;
    let validator = utils::test_pubkey(
        "withdraw_through_delegated_shuttle_with_merge_stores_transfer_and_cleanup_actions::validator",
    );
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let (shuttle_metadata, _) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner.pubkey(), mint, shuttle_id);
    let (shuttle_eata, _) = utils::derive_shuttle_eata(PROGRAM, shuttle_metadata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);
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
    let _setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;
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
            ix_create_owner_source,
            ix_init_owner_source,
            ix_mint_owner_source,
        ],
        Some(&payer),
        &[&payer_kp, &owner_token],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let (buffer_pda, _) =
        Pubkey::find_program_address(&[b"buffer", shuttle_eata.as_ref()], &PROGRAM.into());
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", shuttle_eata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", shuttle_eata.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let mut withdraw_data = vec![instruction::WITHDRAW_THROUGH_DELEGATED_SHUTTLE_WITH_MERGE];
    withdraw_data.extend_from_slice(&shuttle_id.to_le_bytes());
    withdraw_data.extend_from_slice(&TRANSFER_AMOUNT.to_le_bytes());
    withdraw_data.extend_from_slice(&validator.to_bytes());

    let ix_withdraw = Instruction {
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
            AccountMeta::new(owner_source_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: withdraw_data,
    };

    let tx_withdraw = Transaction::new_signed_with_payer(
        &[ix_withdraw],
        Some(&payer),
        &[&payer_kp, &owner],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_withdraw,
        "wd_shuttle::withdraw",
    )
    .await
    .unwrap();

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_metadata)
        .await
        .unwrap()
        .expect("shuttle metadata must exist");
    let mut shuttle_data = shuttle_account.data.clone();
    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleMetadata>(shuttle_data.as_mut_slice()).unwrap() };
    assert!(shuttle.is_initialized());
    assert_eq!(shuttle.owner.as_array(), &owner.pubkey().to_bytes());
    assert_eq!(shuttle.payer.as_array(), &rent_pda.to_bytes());

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
    assert_eq!(shuttle_eata_state.amount, 0);

    let owner_source_account = context
        .banks_client
        .get_account(owner_source_ata)
        .await
        .unwrap()
        .expect("owner source token account must exist");
    let owner_source_state = SplAccount::unpack(&owner_source_account.data).unwrap();
    assert_eq!(owner_source_state.amount, STARTING_BALANCE);

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
        delegation_record_account.data.len() > record_len,
        "expected stored post-delegation payload bytes"
    );
}

#[tokio::test]
async fn undelegate_withdraw_and_close_shuttle_ephemeral_ata_schedules_close_action() {
    let magic_program =
        Pubkey::new_from_array(ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes());
    let magic_context = convert_magic_pubkey(MAGIC_CONTEXT_PUBKEY);
    clear_captured_intent_bundles(magic_program);

    let owner = utils::test_keypair(
        "undelegate_withdraw_and_close_shuttle_ephemeral_ata_schedules_close_action::owner",
    );
    let mint_kp = utils::test_keypair(
        "undelegate_withdraw_and_close_shuttle_ephemeral_ata_schedules_close_action::mint",
    );
    let mint = mint_kp.pubkey();
    let shuttle_id = 3_u32;
    let (vault, _) = Pubkey::find_program_address(&[mint.as_ref()], &PROGRAM);
    let vault_ata = utils::derive_associated_token_address(vault, mint);
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);
    let (shuttle_metadata, _) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner.pubkey(), mint, shuttle_id);
    let (shuttle_eata, _) = utils::derive_shuttle_eata(PROGRAM, shuttle_metadata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);

    let mut shuttle_data = vec![0u8; ShuttleMetadata::LEN];
    let shuttle_state =
        unsafe { load_mut_unchecked::<ShuttleMetadata>(shuttle_data.as_mut_slice()).unwrap() };
    shuttle_state.owner = owner.pubkey();
    shuttle_state.payer = rent_pda;
    shuttle_state.id = shuttle_id;

    let mut shuttle_eata_data = vec![0u8; EphemeralAta::LEN];
    let shuttle_eata_state =
        unsafe { load_mut_unchecked::<EphemeralAta>(shuttle_eata_data.as_mut_slice()).unwrap() };
    shuttle_eata_state.owner = shuttle_metadata;
    shuttle_eata_state.mint = mint;
    shuttle_eata_state.amount = 0;

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        add_magic_program_mock(pt, magic_program);
        pt.add_account(
            rent_pda,
            Account {
                lamports: Rent::default().minimum_balance(0).max(1),
                data: vec![],
                owner: solana_system_interface::program::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            shuttle_metadata,
            Account {
                lamports: Rent::default().minimum_balance(ShuttleMetadata::LEN).max(1),
                data: shuttle_data,
                owner: PROGRAM,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            shuttle_eata,
            Account {
                lamports: Rent::default().minimum_balance(EphemeralAta::LEN).max(1),
                data: shuttle_eata_data,
                owner: PROGRAM,
                executable: false,
                rent_epoch: 0,
            },
        );
        pt.add_account(
            magic_context,
            Account {
                lamports: 1_000_000,
                data: vec![0; 8],
                owner: magic_program,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();

    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;
    let destination_ata = setup.user_tokens[0];

    let ix_create_shuttle_wallet =
        associated_token_create_idempotent_ix(payer, shuttle_wallet_ata, shuttle_metadata, mint);
    let tx_create_shuttle_wallet = Transaction::new_signed_with_payer(
        &[ix_create_shuttle_wallet],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_create_shuttle_wallet,
        "ud_wd_close::wallet_ata",
    )
    .await
    .unwrap();

    let ix_undelegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new_readonly(shuttle_metadata, false),
            AccountMeta::new_readonly(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new(magic_context, false),
            AccountMeta::new_readonly(magic_program, false),
        ],
        data: vec![internal::UNDELEGATE_WITHDRAW_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA],
    };
    let tx_undelegate = Transaction::new_signed_with_payer(
        &[ix_undelegate],
        Some(&payer),
        &[&payer_kp],
        context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_undelegate,
        "ud_wd_close::undelegate",
    )
    .await
    .unwrap();

    let captured_bundles = peek_captured_intent_bundles(magic_program);
    assert_eq!(captured_bundles.len(), 1);
    let commit_and_undelegate = captured_bundles[0]
        .args
        .commit_and_undelegate
        .as_ref()
        .expect("expected commit_and_undelegate bundle");
    let UndelegateTypeArgs::WithBaseActions { base_actions } =
        &commit_and_undelegate.undelegate_type
    else {
        panic!("expected undelegate base actions");
    };
    assert_eq!(base_actions.len(), 1);

    let close_action = &base_actions[0];
    assert_eq!(
        close_action.args.data,
        vec![internal::CLOSE_SHUTTLE_ATA_INTENT, u8::MAX]
    );
    assert_eq!(close_action.accounts.len(), 9);
    assert_eq!(
        close_action.accounts[0].pubkey.to_bytes(),
        rent_pda.to_bytes()
    );
    assert_eq!(
        close_action.accounts[4].pubkey.to_bytes(),
        destination_ata.to_bytes()
    );
    assert_eq!(close_action.accounts[5].pubkey.to_bytes(), mint.to_bytes());
    assert_eq!(close_action.accounts[6].pubkey.to_bytes(), vault.to_bytes());
    assert_eq!(
        close_action.accounts[7].pubkey.to_bytes(),
        vault_ata.to_bytes()
    );

    assert_eq!(
        captured_bundles[0].schedule_accounts[close_action.escrow_authority as usize],
        payer
    );
}
