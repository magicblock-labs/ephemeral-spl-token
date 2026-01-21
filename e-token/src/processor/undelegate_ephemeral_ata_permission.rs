use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::commit_and_undelegate_permission,
    pda::permission_pda_from_permissioned_account,
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

/// Commit and undelegate the permission PDA associated with an Ephemeral ATA.
///
/// Expected accounts:
/// 0. [signer]   Payer (authority)
/// 1. [writable] Ephemeral ATA account (permissioned account)
/// 2. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
/// 3. []         Permission program (ACL)
/// 4. []         Delegation program (magic program)
/// 5. [writable] Magic context account
pub fn process_undelegate_ephemeral_ata_permission(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer_info, ephemeral_ata_info, permission_info, permission_program, magic_program, magic_context, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if *permission_program.address() != PERMISSION_PROGRAM_ID {
        return Err(ProgramError::InvalidAccountData);
    }

    let ephemeral_ata =
        unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked())? };

    if !ephemeral_ata.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }

    if ephemeral_ata.owner != *payer_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    if expected_permission != *permission_info.address() {
        return Err(ProgramError::InvalidSeeds);
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
