use ephemeral_spl_api::{
    require, require_eq_keys, require_n_accounts,
    state::transfer_queue::{queue_views_checked, TransferQueue, ITEM_LEN},
};
use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};

pub const MAX_ITEMS_PER_REALLOC: usize = 10_240 / ITEM_LEN;

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator]).
///  1: []                  - Builtin : System program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_allocate_transfer_queue(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        queue_info, // force multi-line
        _system_program_info,
    ] = require_n_accounts!(accounts, 2);

    require!(
        queue_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let realloc_size = {
        let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
        let derived_queue =
            TransferQueue::derive_pda(&header.mint, &header.validator, header.bump)?;
        require_eq_keys!(
            &derived_queue,
            queue_info.address(),
            ProgramError::InvalidSeeds
        );

        let rent = Rent::get()?;
        let cost_per_item = rent.try_minimum_balance(ITEM_LEN)? - rent.try_minimum_balance(0)?;
        let remaining_capacity =
            queue_info.lamports() - rent.try_minimum_balance(queue_info.data_len())?;
        let current_items = remaining_capacity / cost_per_item;
        current_items.min(MAX_ITEMS_PER_REALLOC as u64) as usize * ITEM_LEN
    };

    queue_info.resize(queue_info.data_len() + realloc_size)
}
