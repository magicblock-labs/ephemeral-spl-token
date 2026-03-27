use ephemeral_spl_api::state::transfer_queue::{item_len, queue_views_checked, QUEUE_SEED};
use pinocchio::address::address_eq;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::assert_owner;

pub const MAX_ITEMS_PER_REALLOC: usize = 10_240 / item_len();

#[inline(always)]
pub fn process_allocate_transfer_queue(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 1. [writable] Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator])
    // 2. []         System program

    let [queue_info, _system_program_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_owner!(queue_info, &ephemeral_spl_api::program::id_address());

    let realloc_size = {
        let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
        let bump_seed = [header.bump];
        let derived_queue = ephemeral_spl_api::Address::create_program_address(
            &[
                QUEUE_SEED,
                header.mint.as_ref(),
                header.validator.as_ref(),
                bump_seed.as_ref(),
            ],
            &ephemeral_spl_api::program::id_address(),
        )?;
        if !address_eq(&derived_queue, queue_info.address()) {
            return Err(ProgramError::InvalidSeeds);
        }

        let rent = Rent::get()?;
        let cost_per_item = rent.try_minimum_balance(item_len())? - rent.try_minimum_balance(0)?;
        let remaining_capacity =
            queue_info.lamports() - rent.try_minimum_balance(queue_info.data_len())?;
        let current_items = remaining_capacity / cost_per_item;
        current_items.min(MAX_ITEMS_PER_REALLOC as u64) as usize * item_len()
    };

    queue_info.resize(queue_info.data_len() + realloc_size)?;

    Ok(())
}
