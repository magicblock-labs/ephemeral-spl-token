use ephemeral_spl_api::{instruction, ID as PROGRAM};
use serial_test::serial;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{program_option::COption, program_pack::Pack as _, rent::Rent, sysvar};
use solana_program_test::{processor, tokio};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_transaction::{InstructionError, Transaction, TransactionError};
use spl_token_interface::state::{Account as SplAccount, AccountState};

mod common;
mod utils;

const MAGIC_PROGRAM: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");
const RENT_PENDING_ATA_CLOSE_AUTHORITY: Pubkey = sysvar::rent::ID;
const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);
const MERGE_AMOUNT: u64 = 700 * 10u64.pow(DECIMALS as u32);

/// Token account shaped like a validator-created rent-pending ATA:
/// zero amount, close authority set to the rent sysvar sentinel.
fn rent_pending_ata_account(owner: Pubkey, mint: Pubkey) -> Account {
    let state = SplAccount {
        mint,
        owner,
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::Some(RENT_PENDING_ATA_CLOSE_AUTHORITY),
    };
    let mut data = vec![0u8; SplAccount::LEN];
    SplAccount::pack(state, &mut data).unwrap();
    Account {
        lamports: Rent::default().minimum_balance(SplAccount::LEN),
        data,
        owner: spl_token_interface::ID,
        executable: false,
        rent_epoch: 0,
    }
}

struct Fixture {
    context: solana_program_test::ProgramTestContext,
    payer_kp: solana_keypair::Keypair,
    mint: Pubkey,
    destination_owner: Pubkey,
    destination_ata: Pubkey,
    shuttle_metadata: Pubkey,
    shuttle_wallet_ata: Pubkey,
}

async fn setup(label: &str, shuttle_id: u32) -> Fixture {
    let destination_owner = utils::test_pubkey(&format!("{label}::destination_owner"));
    let mint_kp = utils::test_keypair(&format!("{label}::mint"));
    let mint = mint_kp.pubkey();

    let destination_ata = utils::derive_associated_token_address(destination_owner, mint);

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.prefer_bpf(false);
        pt.add_program("magic_mock", MAGIC_PROGRAM, processor!(common::magic_mock::process));
        pt.prefer_bpf(true);
        // Pre-created so the instruction exercises the idempotent path: the mock
        // magic program cannot create foreign-owned accounts.
        pt.add_account(destination_ata, rent_pending_ata_account(destination_owner, mint));
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let owner = payer;

    let (shuttle_metadata, _) =
        ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata::find_pda(&owner, &mint, shuttle_id);
    let (shuttle_eata, _) = ephemeral_rollups_pinocchio::spl::EphemeralAta::find_pda(&shuttle_metadata, &mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_metadata, mint);

    utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, STARTING_BALANCE, 1).await;

    let mut shuttle_init_data = instruction::ESplInstruction::InitializeShuttleEphemeralAta.to_vec();
    shuttle_init_data.extend_from_slice(&shuttle_id.to_le_bytes());
    let ix_init_shuttle = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle_metadata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: shuttle_init_data,
    };
    let mut ix_fund_shuttle = spl_token_interface::instruction::mint_to(
        &spl_token_interface::ID,
        &mint,
        &shuttle_wallet_ata,
        &payer,
        &[],
        MERGE_AMOUNT,
    )
    .unwrap();
    ix_fund_shuttle.program_id = spl_token_interface::ID;

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_shuttle, ix_fund_shuttle],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx_init).await.unwrap();

    Fixture {
        context,
        payer_kp,
        mint,
        destination_owner,
        destination_ata,
        shuttle_metadata,
        shuttle_wallet_ata,
    }
}

fn merge_ix(fixture: &Fixture, destination_ata: Pubkey) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(fixture.payer_kp.pubkey(), true),
            AccountMeta::new_readonly(fixture.destination_owner, false),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(fixture.shuttle_metadata, false),
            AccountMeta::new(fixture.shuttle_wallet_ata, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: instruction::ESplInstruction::MergeShuttleIntoRentPendingAta.to_vec(),
    }
}

#[tokio::test]
#[serial]
async fn merge_shuttle_into_rent_pending_ata_creates_ata_and_merges() {
    common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);

    let fixture = setup("merge_rent_pending", 7).await;
    let payer = fixture.payer_kp.pubkey();

    let tx = Transaction::new_signed_with_payer(
        &[merge_ix(&fixture, fixture.destination_ata)],
        Some(&payer),
        &[&fixture.payer_kp],
        fixture.context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&fixture.context.banks_client, tx, "merge_rent_pending::merge")
        .await
        .unwrap();

    let destination_account = fixture
        .context
        .banks_client
        .get_account(fixture.destination_ata)
        .await
        .unwrap()
        .expect("destination ata must exist");
    let destination_state = SplAccount::unpack(&destination_account.data).unwrap();
    assert_eq!(destination_state.amount, MERGE_AMOUNT);
    assert_eq!(destination_state.owner, fixture.destination_owner);

    let shuttle_wallet_account = fixture
        .context
        .banks_client
        .get_account(fixture.shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_state = SplAccount::unpack(&shuttle_wallet_account.data).unwrap();
    assert_eq!(shuttle_wallet_state.amount, 0);

    let creates = common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].wallet_owner, fixture.destination_owner);
    assert_eq!(creates[0].mint, fixture.mint);
    assert_eq!(creates[0].token_program, spl_token_interface::ID);
    assert_eq!(creates[0].accounts[0], payer);
    assert_eq!(creates[0].accounts[1], fixture.destination_ata);

    // Idempotent re-run: the shuttle is drained and the ATA already exists.
    // The extra self-transfer differentiates the signature from the first tx.
    let tx_rerun = Transaction::new_signed_with_payer(
        &[
            merge_ix(&fixture, fixture.destination_ata),
            solana_system_interface::instruction::transfer(&payer, &payer, 1),
        ],
        Some(&payer),
        &[&fixture.payer_kp],
        fixture.context.banks_client.get_latest_blockhash().await.unwrap(),
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx_rerun)
        .await
        .unwrap();

    let destination_account = fixture
        .context
        .banks_client
        .get_account(fixture.destination_ata)
        .await
        .unwrap()
        .expect("destination ata must exist");
    let destination_state = SplAccount::unpack(&destination_account.data).unwrap();
    assert_eq!(destination_state.amount, MERGE_AMOUNT);

    let creates = common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 1, "re-run must still issue the idempotent create CPI");
}

#[tokio::test]
#[serial]
async fn merge_shuttle_into_rent_pending_ata_rejects_mismatched_destination_ata() {
    common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);

    let fixture = setup("merge_rent_pending_mismatch", 8).await;
    let payer = fixture.payer_kp.pubkey();

    let wrong_ata = utils::test_pubkey("merge_rent_pending_mismatch::wrong_ata");
    let tx = Transaction::new_signed_with_payer(
        &[merge_ix(&fixture, wrong_ata)],
        Some(&payer),
        &[&fixture.payer_kp],
        fixture.context.last_blockhash,
    );
    let error = fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .expect_err("mismatched destination ata must fail");
    assert_eq!(
        error.unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidSeeds)
    );

    assert!(common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM).is_empty());
}
