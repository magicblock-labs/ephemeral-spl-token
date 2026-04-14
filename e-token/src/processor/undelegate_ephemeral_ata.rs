use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use pinocchio::{address::address_eq, error::ProgramError, AccountView, ProgramResult};

use crate::processor::utils::validate_token_account;

fn commit_and_undelegate_accounts(
    payer: &AccountView,
    accounts: &[AccountView],
    magic_context: &AccountView,
    magic_program: &AccountView,
) -> ProgramResult {
    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        accounts,
        magic_context,
        magic_program,
        None,
        None,
    )
}

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - SPL     : User ATA account.
///  2: []                  - PDA     : Ephemeral ATA account (PDA derived from [payer, mint]).
///  3: [writable]          - Any     : Magic context account.
///  4: []                  - Program : Delegation program ID.
///
/// Instruction Data: None
///
pub fn process_undelegate_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer, ata_info, ephemeral_ata_info, magic_context, magic_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Ensure the payer signed the transaction
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Read the Ephemeral ATA to get the mint and verify the PDA derivation for this payer.
    // Scope the borrow so it's released before any CPI.
    let (mint, bump) = {
        let eata_data =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        #[allow(clippy::clone_on_copy)]
        let mint = eata_data.mint.clone();
        (mint, eata_data.bump)
    };

    // Derive PDA: seeds = [payer, mint], program id = e-token program id (ephemeral_spl_api::program::ID)
    let derived_pda = EphemeralAta::derive_pda(payer.address(), &mint, bump)?;

    if !address_eq(&derived_pda, ephemeral_ata_info.address()) {
        return Err(ProgramError::InvalidSeeds);
    }

    // Validate that the provided ATA account is a valid SPL token account for [payer, mint].
    validate_token_account(ata_info, &mint, Some(payer.address()), None)?;

    // Commit and undelegate with the user's ATA and the ephemeral ATA as the account set
    commit_and_undelegate_accounts(payer, &[ata_info.clone()], magic_context, magic_program)
}
