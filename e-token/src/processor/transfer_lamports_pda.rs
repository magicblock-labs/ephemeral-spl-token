use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::lamports_pda::{derive_lamports_pda, parse_amount_and_salt};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Lamports PDA account.
///  2: [writable]          - Any     : Destination account.
///
/// Instruction Data: amount (u64) + salt ([u8; 32])
///
#[inline(never)]
pub fn process_transfer_lamports_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        lamports_pda_info,
        destination_info,
    ] = require_n_accounts!(accounts, 3);

    let (amount, salt) = parse_amount_and_salt(instruction_data)?;

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        lamports_pda_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    require!(
        lamports_pda_info.data_len() == 0,
        ProgramError::InvalidAccountData
    );

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    require_eq_keys!(
        &derived_lamports_pda,
        lamports_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    let expected_balance = Rent::get()?
        .try_minimum_balance(0)?
        .checked_add(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    require!(
        lamports_pda_info.lamports() >= expected_balance,
        ProgramError::InvalidArgument
    );

    transfer_lamports(lamports_pda_info, destination_info, amount)
}

fn transfer_lamports(
    source: &AccountView,
    destination: &AccountView,
    amount: u64,
) -> ProgramResult {
    require!(
        source.address() != destination.address(),
        ProgramError::InvalidArgument
    );

    let updated_source_lamports = source
        .lamports()
        .checked_sub(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    let updated_destination_lamports = destination
        .lamports()
        .checked_add(amount)
        .ok_or(ProgramError::InvalidArgument)?;

    source.set_lamports(updated_source_lamports);
    destination.set_lamports(updated_destination_lamports);
    Ok(())
}
