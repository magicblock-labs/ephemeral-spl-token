use ephemeral_spl_api::require_n_accounts;
use pinocchio::{AccountView, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: []                  - PDA     : Delegated PDA account to be restored.
///  1: [signer]            - PDA     : Undelegate buffer PDA.
///  2: []                  - Any     : Payer / original authority.
///  3: []                  - Builtin : System program.
///
/// Instruction Data: delegation-program undelegation callback payload
///
pub fn process_undelegation_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        delegated_acc, // force multi-line
        buffer_acc,
        payer,
        _system_program,
    ] = require_n_accounts!(accounts, 4);

    ephemeral_rollups_pinocchio::instruction::undelegate(
        delegated_acc,
        &crate::ID,
        buffer_acc,
        payer,
        &instruction_data[7..],
    )
}
