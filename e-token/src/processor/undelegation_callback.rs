use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};

/// Undelegation callback invoked by the delegation program.
///
/// Expected accounts (in order used below):
/// 0. []         Payer (original authority for the delegated PDA)
/// 1. [writable] Delegated PDA account to be restored (Ephemeral ATA PDA)
/// 2. []         Owner program (this program ID)
/// 3. [signer]   Undelegate buffer PDA (holds the snapshot of the delegated account)
/// 4. []         System program
pub fn process_undelegation_callback(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let [delegated_acc, buffer_acc, payer, _system_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    ephemeral_rollups_pinocchio::instruction::undelegate(
        delegated_acc,
        &crate::ID,
        buffer_acc,
        payer,
        &instruction_data[7..],
    )
}
