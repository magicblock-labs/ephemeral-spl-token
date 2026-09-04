use ephemeral_spl_api::{error::EphemeralSplError, instruction, ID as PROGRAM};
use serial_test::serial;
use solana_account::Account as SolanaAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::instruction::InstructionError;
use solana_program_test::{tokio, ProgramTest, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::{Transaction, TransactionError};

use crate::common::magic_mock::{self, clear_all_captured, take_captured_commits};

mod common;
mod utils;

struct Fixture {
    context: ProgramTestContext,
    payer_kp: Keypair,
    payer: Pubkey,
    ephemeral_ata: Pubkey,
    /// A real SPL token account for `[payer, mint]`. The program validates this by
    /// parsing it, not by deriving the ATA address, so any such account is accepted.
    user_ata: Pubkey,
    magic_fee_vault: Pubkey,
}

/// Context with the Magic mock registered, a funded mint and token account, and an
/// initialized eATA for `[payer, mint]`.
async fn setup(label: &str) -> Fixture {
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();

    let mint_kp = utils::test_keypair(&format!("{label}::mint"));
    let mint = mint_kp.pubkey();
    let validator = utils::test_pubkey(&format!("{label}::validator"));
    let magic_fee_vault = utils::magic_fee_vault_pda(validator);

    let mut context = utils::start_program_test_with(PROGRAM, |pt: &mut ProgramTest| {
        magic_mock::add_mock(pt);

        pt.add_account(
            utils::magic_context_id(),
            SolanaAccount {
                lamports: 1_000_000,
                data: vec![0u8; 8],
                owner: utils::magic_program_id(),
                executable: false,
                rent_epoch: 0,
            },
        );

        pt.add_account(
            magic_fee_vault,
            SolanaAccount {
                lamports: 1_000_000,
                data: vec![],
                owner: utils::delegation_program_id(),
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    clear_all_captured(utils::magic_program_id());

    let pdas = utils::derive_pdas(PROGRAM, payer, mint);
    let setup = utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 1_000, 1).await;

    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(pdas.ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: instruction::ESplInstruction::InitializeEphemeralAta.to_vec(),
    };

    let tx = Transaction::new_signed_with_payer(&[ix_init_ata], Some(&payer), &[&payer_kp], context.last_blockhash);
    context.banks_client.process_transaction(tx).await.unwrap();

    Fixture {
        context,
        payer_kp,
        payer,
        ephemeral_ata: pdas.ephemeral_ata,
        user_ata: setup.user_tokens[0],
        magic_fee_vault,
    }
}

/// Build instruction 5. `trailing` appends past the five required accounts, so the
/// same builder produces the supported six-account form and the unsupported longer
/// ones.
fn undelegate_ix(fixture: &Fixture, trailing: &[AccountMeta]) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new_readonly(fixture.payer, true),
        AccountMeta::new(fixture.user_ata, false),
        AccountMeta::new_readonly(fixture.ephemeral_ata, false),
        AccountMeta::new(utils::magic_context_id(), false),
        AccountMeta::new_readonly(utils::magic_program_id(), false),
    ];
    accounts.extend_from_slice(trailing);

    Instruction {
        program_id: PROGRAM,
        accounts,
        data: instruction::ESplInstruction::UndelegateEphemeralAta.to_vec(),
    }
}

async fn send(fixture: &mut Fixture, ix: Instruction) -> Result<(), TransactionError> {
    let blockhash = fixture.context.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        std::slice::from_ref(&ix),
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .map_err(|e| e.unwrap())
}

/// The five-account form is what the published program accepts, so it has to keep
/// committing with no fee vault and no shifted account list.
#[tokio::test]
#[serial]
async fn undelegate_ephemeral_ata_without_fee_vault() {
    let mut fixture = setup("undelegate_ephemeral_ata_without_fee_vault").await;

    let ix = undelegate_ix(&fixture, &[]);
    send(&mut fixture, ix).await.unwrap();

    let commits = take_captured_commits(utils::magic_program_id());
    assert_eq!(commits.len(), 1, "expected exactly one commit CPI");
    assert!(
        !commits[0].explicit_fee_vault,
        "no vault means the implicit instruction variant"
    );
    assert_eq!(
        commits[0].accounts,
        vec![fixture.payer, utils::magic_context_id(), fixture.user_ata],
        "no fee vault means the committed account sits directly after the magic context"
    );
}

/// With a sixth account the program must switch to the explicit-fee-vault magic
/// instruction variant, whose vault slot the magic program validates itself.
#[tokio::test]
#[serial]
async fn undelegate_ephemeral_ata_with_fee_vault() {
    let mut fixture = setup("undelegate_ephemeral_ata_with_fee_vault").await;

    let ix = undelegate_ix(&fixture, &[AccountMeta::new(fixture.magic_fee_vault, false)]);
    send(&mut fixture, ix).await.unwrap();

    let commits = take_captured_commits(utils::magic_program_id());
    assert_eq!(commits.len(), 1, "expected exactly one commit CPI");
    assert!(
        commits[0].explicit_fee_vault,
        "the vault must ride the explicit instruction variant"
    );
    assert_eq!(
        commits[0].accounts,
        vec![
            fixture.payer,
            utils::magic_context_id(),
            fixture.magic_fee_vault,
            fixture.user_ata,
        ],
        "the fee vault is positional: third, ahead of the committed account"
    );
}

/// One optional account, not an open tail. A seventh account is a caller mistake and
/// silently ignoring it would let a wrong account list look like a working one.
#[tokio::test]
#[serial]
async fn undelegate_ephemeral_ata_rejects_a_seventh_account() {
    let mut fixture = setup("undelegate_ephemeral_ata_rejects_a_seventh_account").await;

    let stray = utils::test_pubkey("undelegate_ephemeral_ata_rejects_a_seventh_account::stray");
    let ix = undelegate_ix(
        &fixture,
        &[
            AccountMeta::new(fixture.magic_fee_vault, false),
            AccountMeta::new_readonly(stray, false),
        ],
    );

    let err = send(&mut fixture, ix).await.unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(EphemeralSplError::TooManyAccountKeys as u32),
        )
    );

    assert!(
        take_captured_commits(utils::magic_program_id()).is_empty(),
        "a rejected instruction must not have reached Magic"
    );
}

#[tokio::test]
#[serial]
async fn undelegate_ephemeral_ata_rejects_readonly_fee_vault() {
    let mut fixture = setup("undelegate_ephemeral_ata_rejects_readonly_fee_vault").await;

    let ix = undelegate_ix(&fixture, &[AccountMeta::new_readonly(fixture.magic_fee_vault, false)]);

    let err = send(&mut fixture, ix).await.unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::PrivilegeEscalation)
    );
    assert!(take_captured_commits(utils::magic_program_id()).is_empty());
}
