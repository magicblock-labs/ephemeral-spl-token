use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    instruction::UpdatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
    types::{Member, MemberFlags, MembersArgs},
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

/// Update the permission PDA members for an Ephemeral ATA.
///
/// Expected accounts:
/// 0. [signer]   Payer (authority)
/// 1. [writable] Ephemeral ATA account (permissioned account)
/// 2. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
/// 3. []         Permission program (ACL)
pub fn process_update_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Instruction data layout:
    // [0] bump
    // [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    let args = UpdateEphemeralAtaPermission::try_from_bytes(instruction_data)?;

    let [payer_info, ephemeral_ata_info, permission_info, permission_program, ..] = accounts else {
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
        return Err(ProgramError::IncorrectAuthority);
    }

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    if expected_permission != *permission_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let members = [Member {
        flags: MemberFlags::from_acl_flag_byte(args.flag_byte()),
        #[allow(clippy::clone_on_copy)]
        pubkey: ephemeral_ata.owner.clone(),
    }];
    let members_args = MembersArgs {
        members: Some(&members),
    };

    UpdatePermissionCpiBuilder::new(
        payer_info,
        ephemeral_ata_info,
        permission_info,
        &PERMISSION_PROGRAM_ID,
    )
    .members(members_args)
    .seeds(&[ephemeral_ata.owner.as_ref(), ephemeral_ata.mint.as_ref()])
    .bump(args.bump())
    .invoke()
}

pub struct UpdateEphemeralAtaPermission<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl UpdateEphemeralAtaPermission<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<UpdateEphemeralAtaPermission, ProgramError> {
        if bytes.len() < 2 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(UpdateEphemeralAtaPermission {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }

    #[inline]
    pub fn flag_byte(&self) -> u8 {
        unsafe { *self.raw.add(1) }
    }
}
