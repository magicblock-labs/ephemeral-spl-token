use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::{
    assert_owner, assert_signer,
    processor::internal::lamports_pda::{derive_lamports_pda, parse_amount_and_salt},
};

#[inline(never)]
pub fn process_transfer_lamports_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let (amount, salt) = parse_amount_and_salt(instruction_data)?;
    let [payer_info, lamports_pda_info, destination_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);
    assert_owner!(lamports_pda_info, &crate::ID);

    if lamports_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    if derived_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let expected_balance = Rent::get()?
        .try_minimum_balance(0)?
        .checked_add(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    if lamports_pda_info.lamports() < expected_balance {
        return Err(ProgramError::InvalidArgument);
    }

    transfer_lamports(lamports_pda_info, destination_info, amount)
}

fn transfer_lamports(
    source: &AccountView,
    destination: &AccountView,
    amount: u64,
) -> ProgramResult {
    if *source.address() == *destination.address() {
        return Err(ProgramError::InvalidArgument);
    }

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
