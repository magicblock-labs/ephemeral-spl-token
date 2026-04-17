use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    instruction::UpdatePermissionCpiBuilder,
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
///  2: [signer]            - Keypair : Owner (must match the Ephemeral ATA owner).
///  3: []                  - Program : Permission program (ACL).
///
/// Instruction Data: ResetEphemeralAtaPermission
///
#[inline(always)]
pub fn process_reset_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        ephemeral_ata_info, // force multi-line
        permission_info,
        owner_info,
        permission_program,
    ] = require_n_accounts!(accounts, 4);

    let args = ResetEphemeralAtaPermission::try_from_bytes(instruction_data)?;

    require!(
        owner_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission_program.address(),
        ProgramError::InvalidAccountData
    );

    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

    require_eq_keys!(
        &ephemeral_ata.owner,
        owner_info.address(),
        ProgramError::IncorrectAuthority
    );

    require!(
        permission_info.lamports() != 0,
        ProgramError::InvalidAccountData
    );

    let mut members_flag = MemberFlags::from_acl_flag_byte(args.flag_byte());
    members_flag.set(MemberFlags::AUTHORITY);
    let members_buf = [Member {
        flags: members_flag,
        #[allow(clippy::clone_on_copy)]
        pubkey: ephemeral_ata.owner.clone(),
    }];
    let members_args = MembersArgs {
        members: Some(&members_buf),
    };

    UpdatePermissionCpiBuilder::new(
        owner_info,
        ephemeral_ata_info,
        permission_info,
        &PERMISSION_PROGRAM_ID,
    )
    .seeds(&[ephemeral_ata.owner.as_ref(), ephemeral_ata.mint.as_ref()])
    .bump(ephemeral_ata.bump)
    .members(members_args)
    .invoke()
}

///
/// DataLayout:
///
///     00..01 : flag_byte (u8)
///
/// ValidLength:
///
///     >= 01
///
pub struct ResetEphemeralAtaPermission<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl ResetEphemeralAtaPermission<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<ResetEphemeralAtaPermission<'_>, ProgramError> {
        require!(!bytes.is_empty(), ProgramError::InvalidInstructionData);

        Ok(ResetEphemeralAtaPermission {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn flag_byte(&self) -> u8 {
        unsafe { *self.raw }
    }
}
