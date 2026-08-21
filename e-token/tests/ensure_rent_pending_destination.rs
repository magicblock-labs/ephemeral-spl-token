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
const TRANSFER_AMOUNT: u64 = 250 * 10u64.pow(DECIMALS as u32);

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
    source_ata: Pubkey,
}

async fn setup(label: &str) -> Fixture {
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

    let setup =
        utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, DECIMALS, STARTING_BALANCE, 1).await;
    let source_ata = setup.user_tokens[0];

    Fixture {
        context,
        payer_kp,
        mint,
        destination_owner,
        destination_ata,
        source_ata,
    }
}

fn ensure_ix(fixture: &Fixture, destination_ata: Pubkey) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(fixture.payer_kp.pubkey(), true),
            AccountMeta::new_readonly(fixture.destination_owner, false),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: instruction::ESplInstruction::EnsureRentPendingDestination.to_vec(),
    }
}

fn transfer_ix(fixture: &Fixture, amount: u64) -> Instruction {
    let mut ix = spl_token_interface::instruction::transfer(
        &spl_token_interface::ID,
        &fixture.source_ata,
        &fixture.destination_ata,
        &fixture.payer_kp.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    ix.program_id = spl_token_interface::ID;
    ix
}

#[tokio::test]
#[serial]
async fn ensure_rent_pending_destination_then_plain_transfer() {
    common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);

    let fixture = setup("ensure_rent_pending").await;
    let payer = fixture.payer_kp.pubkey();

    let tx = Transaction::new_signed_with_payer(
        &[
            ensure_ix(&fixture, fixture.destination_ata),
            transfer_ix(&fixture, TRANSFER_AMOUNT),
        ],
        Some(&payer),
        &[&fixture.payer_kp],
        fixture.context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(&fixture.context.banks_client, tx, "ensure_rent_pending::transfer")
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
    assert_eq!(destination_state.amount, TRANSFER_AMOUNT);
    assert_eq!(destination_state.owner, fixture.destination_owner);

    let source_account = fixture
        .context
        .banks_client
        .get_account(fixture.source_ata)
        .await
        .unwrap()
        .expect("source ata must exist");
    let source_state = SplAccount::unpack(&source_account.data).unwrap();
    assert_eq!(source_state.amount, STARTING_BALANCE - TRANSFER_AMOUNT);

    let creates = common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].wallet_owner, fixture.destination_owner);
    assert_eq!(creates[0].mint, fixture.mint);
    assert_eq!(creates[0].token_program, spl_token_interface::ID);
    assert_eq!(creates[0].accounts[1], fixture.destination_ata);

    // Idempotent re-run against the now-funded destination.
    let tx_rerun = Transaction::new_signed_with_payer(
        &[
            ensure_ix(&fixture, fixture.destination_ata),
            transfer_ix(&fixture, TRANSFER_AMOUNT),
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
    assert_eq!(destination_state.amount, 2 * TRANSFER_AMOUNT);
}

#[tokio::test]
#[serial]
async fn ensure_rent_pending_destination_rejects_mismatched_ata() {
    common::magic_mock::take_captured_rent_pending_ata_creates(MAGIC_PROGRAM);

    let fixture = setup("ensure_rent_pending_mismatch").await;
    let payer = fixture.payer_kp.pubkey();

    let wrong_ata = utils::test_pubkey("ensure_rent_pending_mismatch::wrong_ata");
    let tx = Transaction::new_signed_with_payer(
        &[ensure_ix(&fixture, wrong_ata)],
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
