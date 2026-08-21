use ephemeral_rollups_pinocchio::{consts::MAGIC_PROGRAM_ID, instruction::CreateRentPendingAta};
use ephemeral_spl_api::{require, require_eq_keys};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::is_supported_token_program;

pub(crate) struct RentPendingDestinationAccounts<'a> {
    pub(crate) payer_info: &'a AccountView,
    pub(crate) destination_owner_info: &'a AccountView,
    pub(crate) destination_ata_info: &'a AccountView,
    pub(crate) mint_info: &'a AccountView,
    pub(crate) token_program_info: &'a AccountView,
    pub(crate) magic_program_info: &'a AccountView,
}

/// Executes on: ER only.
///
/// Idempotently creates the destination's ATA as a rent-pending ATA through
/// the Magic program, which derive-checks the ATA against the destination
/// owner so mismatched accounts fail closed. Read access is owner-gated by
/// the private RPC's token-account default; no permission account is created.
pub(crate) fn ensure_rent_pending_destination(accounts: &RentPendingDestinationAccounts<'_>) -> ProgramResult {
    require!(
        is_supported_token_program(accounts.token_program_info.address()),
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(
        &MAGIC_PROGRAM_ID,
        accounts.magic_program_info.address(),
        ProgramError::IncorrectProgramId
    );

    let destination_owner = accounts.destination_owner_info.address();

    CreateRentPendingAta {
        payer: accounts.payer_info,
        ata: accounts.destination_ata_info,
        mint: accounts.mint_info,
        token_program: accounts.token_program_info,
        magic_program: accounts.magic_program_info,
        wallet_owner: destination_owner,
    }
    .invoke()
}
