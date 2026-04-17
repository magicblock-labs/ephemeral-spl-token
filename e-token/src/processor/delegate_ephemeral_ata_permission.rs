use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::DelegatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{cpi::Signer, error::ProgramError, AccountView, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer (authority).
///  1: [writable]          - PDA     : Ephemeral ATA account (permissioned account).
///  2: []                  - Program : Permission program (ACL).
///  3: [writable]          - PDA     : Permission PDA (derived from ["permission:", ephemeral_ata]).
///  4: []                  - Builtin : System program.
///  5: [writable]          - PDA     : Delegation buffer PDA.
///  6: [writable]          - PDA     : Delegation record PDA.
///  7: [writable]          - PDA     : Delegation metadata PDA.
///  8: []                  - Program : Delegation program.
///  9: []                  - Any     : Validator.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_delegate_ephemeral_ata_permission(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        ephemeral_ata_info,
        permission_program,
        permission_info,
        system_program,
        delegation_buffer,
        delegation_record,
        delegation_metadata,
        delegation_program,
        validator,
    ] = require_n_accounts!(accounts, 10);

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission_program.address(),
        ProgramError::InvalidAccountData
    );

    let dlp_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;

    if permission_info.owned_by(&dlp_program) {
        return Ok(());
    }

    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());

    require_eq_keys!(
        &expected_permission,
        permission_info.address(),
        ProgramError::InvalidSeeds
    );

    let bump = [ephemeral_ata.bump];
    let seeds = EphemeralAta::signer_seeds(&ephemeral_ata.owner, &ephemeral_ata.mint, &bump);
    let signer_seeds = Signer::from(&seeds);

    DelegatePermissionCpiBuilder::new(
        payer_info,
        payer_info,
        ephemeral_ata_info,
        permission_info,
        system_program,
        permission_program,
        delegation_buffer,
        delegation_record,
        delegation_metadata,
        delegation_program,
        validator,
        &PERMISSION_PROGRAM_ID,
    )
    .signer_seeds(signer_seeds)
    .invoke()
}
