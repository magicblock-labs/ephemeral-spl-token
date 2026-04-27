use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    instruction::CreatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
    types::{Member, MemberFlags, MembersArgs},
};
use ephemeral_spl_api::{require, require_eq_keys};
use ephemeral_spl_api::{
    require_n_accounts,
    state::{ephemeral_ata::EphemeralAta, load_initialized},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Ephemeral ATA account (PDA derived from [owner, mint]).
///  1: [writable]          - PDA     : Permission PDA (derived from ["permission:", ephemeral_ata]).
///  2: [signer]            - Keypair : Payer (must match the Ephemeral ATA owner).
///  3: []                  - Builtin : System program.
///  4: []                  - Program : Permission program (ACL).
///
/// Instruction Data: CreateEphemeralAtaPermissionArgs
///
#[inline(always)]
pub fn process_create_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        ephemeral_ata_info, // force multi-line
        permission_info,
        payer_info,
        system_program,
        permission_program,
    ] = require_n_accounts!(accounts, 5);

    let args = CreateEphemeralAtaPermissionArgs::decode(instruction_data)?;

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission_program.address(),
        ProgramError::InvalidAccountData
    );

    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

    let flag_byte = args.flag_byte();

    // Valid in 2 cases:
    // - Payer is the owner of the eata
    // - Permisionless, but permission are default (only readable for eata owner)
    require!(
        ephemeral_ata.owner == *payer_info.address() || flag_byte == 0,
        ProgramError::IncorrectAuthority
    );

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    require_eq_keys!(
        &expected_permission,
        permission_info.address(),
        ProgramError::InvalidSeeds
    );

    // Idempotent create: if the permission account already exists, return Ok(())
    // for safe transaction batching rather than treating it as an error.
    if permission_info.lamports() > 0 {
        return Ok(());
    }

    let mut members_flag = MemberFlags::from_acl_flag_byte(flag_byte);
    members_flag.set(MemberFlags::AUTHORITY);
    let members_buf = [Member {
        flags: members_flag,
        #[allow(clippy::clone_on_copy)]
        pubkey: ephemeral_ata.owner.clone(),
    }];
    let members_args = MembersArgs {
        members: Some(&members_buf),
    };

    let builder = CreatePermissionCpiBuilder::new(
        ephemeral_ata_info,
        permission_info,
        payer_info,
        system_program,
        &PERMISSION_PROGRAM_ID,
    );
    builder
        .seeds(&[ephemeral_ata.owner.as_ref(), ephemeral_ata.mint.as_ref()])
        .bump(ephemeral_ata.bump)
        .members(members_args)
        .invoke()
}

#[variable_offset_layout(buffer_offset = 1)]
pub struct CreateEphemeralAtaPermissionArgs {
    flag_byte: u8,
}
