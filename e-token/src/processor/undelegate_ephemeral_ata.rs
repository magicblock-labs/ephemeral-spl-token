use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
use ephemeral_spl_api::{
    error::EphemeralSplError,
    require, require_eq_keys, require_n_accounts_with_optionals, require_owned_by,
    state::{ephemeral_ata::EphemeralAta, load_initialized},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::{validate_magic_accounts, validate_token_account};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - SPL     : User ATA account.
///  2: []                  - PDA     : Ephemeral ATA account (PDA derived from [payer, mint]).
///  3: [writable]          - Any     : Magic context account.
///  4: []                  - Program : Magic program ID.
///  5: [writable, optional] - PDA     : Magic fee vault of the executing
///                                      validator. Required when the payer is
///                                      itself delegated; validated by the
///                                      magic program.
///
pub fn process_undelegate_ephemeral_ata(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    let (required, optional) = require_n_accounts_with_optionals!(accounts, 5);
    let [
        payer, // force multi-line
        ata_info,
        ephemeral_ata_info,
        magic_context,
        magic_program,
    ] = required;
    let magic_fee_vault = match optional {
        [] => None,
        [vault] => Some(vault),
        _ => return Err(EphemeralSplError::TooManyAccountKeys.into()),
    };

    // Ensure the payer signed the transaction
    require!(payer.is_signer(), ProgramError::MissingRequiredSignature);
    validate_magic_accounts(magic_context, magic_program)?;

    // Read the Ephemeral ATA to get the mint and verify the PDA derivation for this payer.
    // Scope the borrow so it's released before any CPI.
    let (mint, bump) = {
        let eata_data = load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        (eata_data.mint, eata_data.bump)
    };

    // Derive PDA: seeds = [payer, mint], program id = e-token program id (ephemeral_spl_api::program::ID)
    let derived_pda = EphemeralAta::derive_pda(payer.address(), &mint, bump)?;

    require_eq_keys!(&derived_pda, ephemeral_ata_info.address(), ProgramError::InvalidSeeds);

    // Validate that the provided ATA account is a valid SPL token account for [payer, mint].
    validate_token_account(ata_info, &mint, Some(payer.address()), None)?;

    // The fee vault slot is the only account this instruction forwards into
    // the commit CPI without validating otherwise, and the magic program
    // trusts its CPI parent for the accounts it passes: for a delegated payer
    // it pins the slot to the executing validator's exact vault, but for a
    // non-delegated payer it treats the slot as one more account to commit,
    // admitting token-program-owned accounts (carve-out) and accounts owned
    // by this program (parent match). Requiring delegation-program ownership
    // excludes both admissible classes, so nothing harmful can occupy the
    // slot; every other delegation-program-owned impostor is rejected by the
    // magic program's own committee and vault checks.
    if let Some(vault) = magic_fee_vault {
        require_owned_by!(vault, &DELEGATION_PROGRAM_ID);
    }

    // Commit and undelegate with the user's ATA and the ephemeral ATA as the account set
    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        core::slice::from_ref(ata_info),
        magic_context,
        magic_program,
        magic_fee_vault,
        None,
    )
}
