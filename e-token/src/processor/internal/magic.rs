use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::require_eq_keys;
use pinocchio::{
    cpi::invoke_signed_with_bounds,
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, ProgramResult,
};

/// Pins the magic context and magic program accounts to their fixed addresses.
#[inline(always)]
pub(crate) fn validate_magic_accounts(magic_context: &AccountView, magic_program: &AccountView) -> ProgramResult {
    require_eq_keys!(
        magic_context.address(),
        &MAGIC_CONTEXT_ID,
        ProgramError::InvalidArgument
    );
    require_eq_keys!(
        magic_program.address(),
        &MAGIC_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );
    Ok(())
}

/// Commit-and-undelegate CPI with an explicit fee vault. Emits
/// `ScheduleCommitAndUndelegateWithFeeVault` (wire tag 27), whose vault slot
/// the magic program validates against the executing validator's fee vault
/// PDA regardless of the payer's delegation state — so no validation is
/// needed here. Drop in favor of the SDK builder once it gains the variant.
pub(crate) fn commit_and_undelegate_with_fee_vault(
    payer: &AccountView,
    ata: &AccountView,
    magic_context: &AccountView,
    magic_program: &AccountView,
    magic_fee_vault: &AccountView,
) -> ProgramResult {
    const DATA: [u8; 4] = 27u32.to_le_bytes();
    let account_metas = [
        InstructionAccount::new(payer.address(), payer.is_writable(), true),
        InstructionAccount::writable(magic_context.address()),
        InstructionAccount::writable(magic_fee_vault.address()),
        InstructionAccount::new(ata.address(), ata.is_writable(), ata.is_signer()),
    ];
    let instruction = InstructionView {
        program_id: magic_program.address(),
        accounts: &account_metas,
        data: &DATA,
    };
    invoke_signed_with_bounds::<4>(&instruction, &[payer, magic_context, magic_fee_vault, ata], &[])
}
