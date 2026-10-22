use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};

use crate::processor::internal::{lamports_pda::derive_lamports_pda, transfer_queue_refill::validate_rent_pda};

///
/// Executes on: BASE only.
///
/// Permissionless recovery for a sponsored lamports PDA that was undelegated
/// without its post-delegation actions running (e.g. the ER rejected them).
/// Refunds everything above the sponsored rent to the payer, returns the rent
/// to the global rent PDA and closes the account. The caller pays the fee.
/// The rent share is the current base minimum for a zero-data account, so a
/// rate reduction after creation shifts the difference to the payer. A payer
/// left below rent exemption must be topped up (same tx is fine) or the
/// runtime rejects the refund.
///
/// Accounts:
///
///  0: [writable]          - Any     : Payer used in the PDA derivation (receives the principal).
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Lamports PDA account.
///  3: []                  - Any     : Destination used in the PDA derivation.
///
/// Instruction Data: salt ([u8; 32])
///
#[inline(never)]
pub fn process_close_lamports_pda(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        lamports_pda_info,
        destination_info,
    ] = require_n_accounts!(accounts, 4);

    let salt: &[u8; 32] = instruction_data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    validate_rent_pda(rent_pda_info)?;
    require!(
        lamports_pda_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(lamports_pda_info.data_len() == 0, ProgramError::InvalidAccountData);

    let (derived_lamports_pda, _) = derive_lamports_pda(payer_info.address(), destination_info.address(), salt);
    require_eq_keys!(
        &derived_lamports_pda,
        lamports_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    let lamports = lamports_pda_info.lamports();
    let rent_refund = Rent::get()?.try_minimum_balance(0)?.min(lamports);
    let principal = lamports.checked_sub(rent_refund).ok_or(ProgramError::InvalidArgument)?;

    let updated_rent_pda_lamports = rent_pda_info
        .lamports()
        .checked_add(rent_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    rent_pda_info.set_lamports(updated_rent_pda_lamports);
    let updated_payer_lamports = payer_info
        .lamports()
        .checked_add(principal)
        .ok_or(ProgramError::InvalidArgument)?;
    payer_info.set_lamports(updated_payer_lamports);
    lamports_pda_info.set_lamports(0);
    lamports_pda_info.close()
}
