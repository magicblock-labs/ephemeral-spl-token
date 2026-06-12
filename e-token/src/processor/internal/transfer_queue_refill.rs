use ephemeral_spl_api::{
    consts::TRANSFER_QUEUE_REFILL_LAMPORTS,
    require, require_eq_keys,
    state::{
        transfer_queue::{queue_views_checked, QUEUE_SEED},
        transfer_queue_refill::derive_transfer_queue_refill_state_address,
    },
};
use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView,
};
use solana_address::Address;

use crate::processor::internal::rent_pda::RENT_PDA;

pub(crate) const MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX: u8 = 1;
// This path may need to create the refill-state PDA on first use, so it needs
// more headroom than a pure flag update.
pub(crate) const MARK_TRANSFER_QUEUE_REFILL_PENDING_COMPUTE_UNITS: u32 = 50_000;

#[inline(always)]
pub(crate) fn refill_transfer_queue_amounts(
    queue_data_len: usize,
) -> Result<(u64, u64), ProgramError> {
    let queue_rent_exemption = Rent::get()?.try_minimum_balance(queue_data_len)?;
    Ok((queue_rent_exemption, TRANSFER_QUEUE_REFILL_LAMPORTS))
}

#[inline(always)]
pub(crate) fn queue_refill_state_address(queue: &Address) -> Address {
    derive_transfer_queue_refill_state_address(queue).0
}

pub(crate) fn validate_queue_account(queue_info: &AccountView) -> Result<(), ProgramError> {
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    require!(
        queue_info.owned_by(&crate::ID) || queue_info.owned_by(&delegation_program),
        ProgramError::IllegalOwner
    );

    let queue_data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(queue_data)?;
    let bump_seed = [header.bump];
    let derived_queue = Address::create_program_address(
        &[
            QUEUE_SEED,
            header.mint.as_ref(),
            header.validator.as_ref(),
            bump_seed.as_ref(),
        ],
        &crate::ID,
    )
    .map_err(|_| ProgramError::InvalidAccountData)?;
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

    Ok(())
}

pub(crate) fn validate_rent_pda(rent_pda_info: &AccountView) -> Result<(), ProgramError> {
    require!(
        rent_pda_info.owned_by(&pinocchio_system::ID),
        ProgramError::InvalidAccountOwner
    );
    require_eq_keys!(
        &RENT_PDA,
        rent_pda_info.address(),
        ProgramError::InvalidSeeds
    );
    require!(
        rent_pda_info.data_len() == 0,
        ProgramError::InvalidAccountData
    );

    Ok(())
}

pub(crate) fn validate_queue_refill_state_address(
    refill_state_info: &AccountView,
    queue: &Address,
) -> Result<(Address, u8), ProgramError> {
    let (expected_refill_state, bump) = derive_transfer_queue_refill_state_address(queue);
    require_eq_keys!(
        &expected_refill_state,
        refill_state_info.address(),
        ProgramError::InvalidSeeds
    );

    Ok((expected_refill_state, bump))
}
