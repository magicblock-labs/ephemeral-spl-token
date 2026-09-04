use ephemeral_spl_api::{
    error::EphemeralSplError,
    require, require_eq_keys, require_n_accounts_with_optionals,
    state::{ephemeral_ata::EphemeralAta, load_initialized},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::{
    commit_and_undelegate_with_fee_vault, validate_magic_accounts, validate_token_account,
};

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

    // Commit and undelegate the user's ATA. With a fee vault the explicit
    // magic instruction variant is used: the magic program validates the
    // vault account itself, so it is forwarded unchecked here.
    match magic_fee_vault {
        Some(vault) => commit_and_undelegate_with_fee_vault(payer, ata_info, magic_context, magic_program, vault),
        None => ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
            payer,
            core::slice::from_ref(ata_info),
            magic_context,
            magic_program,
            None,
            None,
        ),
    }
}
