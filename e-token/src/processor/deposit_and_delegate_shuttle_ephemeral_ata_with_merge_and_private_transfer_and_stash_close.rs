use ephemeral_spl_api::instructions::DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs;
use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::{require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::internal::private_transfer::process_with_merge_and_private_transfer_inner;
use crate::processor::internal::shuttle_delegation::CloseStashArgs;

///
/// Executes on: BASE only. Self-CPI'd by `ExecuteScheduledPrivateTransfer`.
///
/// Same accounts (19) and semantics as ix 25, with a fixed `[u8; 33]`
/// `stash_close_seeds` appended to the args. The post-undelegate settlement
/// closes the source stash ATA and refunds the stash PDA to the rent PDA.
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer_and_stash_close(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args =
        DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs::decode(instruction_data)?;

    let _ = require_n_accounts!(accounts, 19);
    let payer_info = &accounts[0];
    let mint_info = &accounts[13];

    let seeds = args.stash_close_seeds();
    let mut user = [0u8; 32];
    user.copy_from_slice(&seeds[0..32]);
    let stash_bump = seeds[32];

    let user_address = Address::new_from_array(user);
    let derived_stash_pda = StashPda::derive_pda(&user_address, mint_info.address(), stash_bump)?;
    require_eq_keys!(
        payer_info.address(),
        &derived_stash_pda,
        ProgramError::InvalidSeeds
    );

    let close_stash = CloseStashArgs { user, stash_bump };

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
