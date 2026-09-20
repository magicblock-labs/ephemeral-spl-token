use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::{require, require_eq_keys};
use pinocchio::{
    cpi::invoke_with_bounds,
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, ProgramResult,
};

use crate::processor::internal::is_supported_token_program;

// TODO: Replace this encoding with `CreateMagicAta` when it is published for Pinocchio 0.10.
const CREATE_MAGIC_ATA_VARIANT: [u8; 4] = [15, 0, 0, 0];

pub(crate) struct MagicAtaDestinationAccounts<'a> {
    pub(crate) payer_info: &'a AccountView,
    pub(crate) destination_owner_info: &'a AccountView,
    pub(crate) destination_ata_info: &'a AccountView,
    pub(crate) mint_info: &'a AccountView,
    pub(crate) token_program_info: &'a AccountView,
    pub(crate) magic_program_info: &'a AccountView,
}

/// Executes on: ER only.
///
/// Idempotently creates the destination's ATA as a Magic ATA through
/// the Magic program, which derive-checks the ATA against the destination
/// owner so mismatched accounts fail closed. Read access is owner-gated by
/// the private RPC's token-account default; no permission account is created.
pub(crate) fn ensure_magic_ata_destination(accounts: &MagicAtaDestinationAccounts<'_>) -> ProgramResult {
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

    let mut data = [0u8; 36];
    data[..4].copy_from_slice(&CREATE_MAGIC_ATA_VARIANT);
    data[4..].copy_from_slice(destination_owner.as_ref());

    let ix_accounts = [
        InstructionAccount::readonly_signer(accounts.payer_info.address()),
        InstructionAccount::writable(accounts.destination_ata_info.address()),
        InstructionAccount::readonly(accounts.mint_info.address()),
        InstructionAccount::readonly(accounts.token_program_info.address()),
    ];

    invoke_with_bounds::<4>(
        &InstructionView {
            program_id: accounts.magic_program_info.address(),
            accounts: &ix_accounts,
            data: &data,
        },
        &[
            accounts.payer_info,
            accounts.destination_ata_info,
            accounts.mint_info,
            accounts.token_program_info,
        ],
    )
}
