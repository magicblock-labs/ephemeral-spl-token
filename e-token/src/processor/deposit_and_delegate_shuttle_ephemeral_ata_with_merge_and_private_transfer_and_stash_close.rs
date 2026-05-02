use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;

use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::{require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer::process_with_merge_and_private_transfer_inner;
use crate::processor::internal::shuttle_delegation::CloseStashArgs;

///
/// Executes on: BASE only. Self-CPI'd by `ExecuteScheduledPrivateTransfer`.
///
/// Identical to `DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer`
/// (ix 25), with one extra fixed `[u8; 33]` field appended to the args. Slot 0
/// (`payer`) and slot 5 (`owner`) must be the stash PDA derived from
/// `[b"stash", user, mint]`. The post-undelegate settlement closes the source
/// stash ATA and drains the stash PDA back to the rent PDA.
///
/// Accounts (19): same layout as ix 25.
///
/// Instruction Data: `DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs`
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_and_stash_close(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args =
        DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs::decode(instruction_data)?;

    // Need slot 0 (payer = stash PDA) and slot 1 (rent_pda) up-front to validate the
    // stash close seeds. The shared inner helper re-validates the rest of the account
    // list after this guard runs.
    let _ = require_n_accounts!(accounts, 19);
    let payer_info = &accounts[0];
    let rent_pda_info = &accounts[1];
    let mint_info = &accounts[13];

    let seeds = args.stash_close_seeds();
    let mut user = [0u8; 32];
    user.copy_from_slice(&seeds[0..32]);
    let stash_bump = seeds[32];

    // SAFETY: `&[u8; 32]` and `&Address` share the same in-memory layout.
    let user_address: &Address = unsafe { &*(user.as_ptr() as *const Address) };

    let derived_stash_pda = StashPda::derive_pda(user_address, mint_info.address(), stash_bump)?;
    require_eq_keys!(
        payer_info.address(),
        &derived_stash_pda,
        ProgramError::InvalidSeeds
    );

    let close_stash = CloseStashArgs {
        stash_pda: payer_info.address(),
        rent_pda: rent_pda_info.address(),
        user,
        stash_bump,
    };

    process_with_merge_and_private_transfer_inner(
        accounts,
        args.shuttle_id(),
        args.amount(),
        args.exact_out(),
        args.validator(),
        args.encrypted_destination(),
        args.encrypted_data_suffix(),
        Some(close_stash),
    )
}

/// Wire-identical to `DepositAndDelegateShuttleWithPrivateTransferArgs` plus a
/// fixed `stash_close_seeds: [u8; 33]` field inserted before
/// `encrypted_data_suffix`. Layout of the new field:
///   bytes [0..32] = user pubkey (the stash PDA seed)
///   byte  [32]    = stash bump
#[variable_offset_layout(buffer_offset = 1)]
pub struct DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub exact_out: bool,
    pub encrypted_destination: [u8; 80],
    pub validator: Option<[u8; 32]>,
    pub stash_close_seeds: [u8; 33],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}
