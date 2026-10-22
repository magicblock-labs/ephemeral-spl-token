use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::require_eq_keys;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

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
