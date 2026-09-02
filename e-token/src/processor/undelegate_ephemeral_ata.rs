use ephemeral_rollups_pinocchio::pda::magic_fee_vault_pda_from_validator;
use ephemeral_spl_api::{
    error::EphemeralSplError,
    instructions::UndelegateArgs,
    require, require_eq_keys, require_n_accounts_with_optionals,
    state::{ephemeral_ata::EphemeralAta, load_initialized},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use wheels::layout::Decodable as _;

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
///  5: [writable, optional] - PDA     : Magic fee vault for the delegating
///                                      validator. Required when the eATA
///                                      owner is itself delegated.
///
/// Instruction Data: UndelegateArgs. The validator identity is required when
/// the magic fee vault account is passed and must be omitted otherwise.
///
pub fn process_undelegate_ephemeral_ata(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
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

    let args = UndelegateArgs::decode(instruction_data)?;

    // Ensure the payer signed the transaction
    require!(payer.is_signer(), ProgramError::MissingRequiredSignature);
    validate_magic_accounts(magic_context, magic_program)?;

    // The fee vault covers commit fees when the payer is delegated. Pin it to
    // the delegation program's fee-vault PDA for the provided validator: for a
    // non-delegated payer the magic program treats this CPI slot as an extra
    // account to commit, so an unchecked account here would let anyone force
    // commit-and-undelegate an arbitrary delegated account. Authenticating the
    // validator itself is the magic program's job — it only ever charges the
    // vault of the validator executing the commit and rejects any other
    // account in this slot, so a wrong validator argument fails cleanly.
    match (magic_fee_vault, args.validator()) {
        (None, None) => {}
        (Some(vault), Some(validator)) => {
            let derived_vault = magic_fee_vault_pda_from_validator(validator);
            require_eq_keys!(&derived_vault, vault.address(), ProgramError::InvalidSeeds);
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }

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
