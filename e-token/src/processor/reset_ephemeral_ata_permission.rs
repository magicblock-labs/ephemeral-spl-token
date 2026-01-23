use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    instruction::UpdatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
    types::{Member, MemberFlags, MembersArgs},
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

#[inline(always)]
pub fn process_reset_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [owner, mint]) - signer via seeds
    // 1. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
    // 2. [signer]   Owner (must match Ephemeral ATA owner)
    // 3. []         Permission program (ACL)

    // Instruction data layout:
    // [0] bump
    // [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    let args = ResetEphemeralAtaPermission::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, permission_info, owner_info, permission_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !owner_info.is_signer() {
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

    if ephemeral_ata.owner != *owner_info.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    // TODO(GabrielePicco): pass bump once supported in the
    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    if expected_permission != *permission_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if permission_info.lamports() == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

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
    .bump(args.bump())
    .members(members_args)
    .invoke()
}

pub struct ResetEphemeralAtaPermission<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl ResetEphemeralAtaPermission<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<ResetEphemeralAtaPermission, ProgramError> {
        if bytes.len() < 2 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(ResetEphemeralAtaPermission {
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
