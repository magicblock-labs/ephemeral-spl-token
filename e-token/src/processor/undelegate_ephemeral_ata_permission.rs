use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::commit_and_undelegate_permission,
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use pinocchio::{address::address_eq, error::ProgramError, AccountView, ProgramResult};

use crate::assert_signer;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer (authority).
///  1: [writable]          - PDA     : Ephemeral ATA account (permissioned account).
///  2: [writable]          - PDA     : Permission PDA (derived from ["permission:", ephemeral_ata]).
///  3: []                  - Program : Permission program (ACL).
///  4: []                  - Program : Delegation program.
///  5: [writable]          - Any     : Magic context account.
///
/// Instruction Data: None
///
pub fn process_undelegate_ephemeral_ata_permission(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer_info, ephemeral_ata_info, permission_info, permission_program, magic_program, magic_context, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);

    if !address_eq(permission_program.address(), &PERMISSION_PROGRAM_ID) {
        return Err(ProgramError::InvalidAccountData);
    }

    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

    if !address_eq(&ephemeral_ata.owner, payer_info.address()) {
        return Err(ProgramError::InvalidAccountData);
    }

    commit_and_undelegate_permission(
        &[
            payer_info,
            ephemeral_ata_info,
            permission_info,
            magic_program,
            magic_context,
        ],
        &PERMISSION_PROGRAM_ID,
        true,
        false,
        None,
    )
}
