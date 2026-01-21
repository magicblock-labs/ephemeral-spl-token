use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    instruction::CreatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
    types::{Member, MemberFlags, MembersArgs},
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

#[inline(always)]
pub fn process_create_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [owner, mint]) - signer via seeds
    // 1. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
    // 2. [signer]   Payer (must match Ephemeral ATA owner)
    // 3. []         System program
    // 4. []         Permission program (ACL)

    // Instruction data layout:
    // [0] bump
    // [1..=5] MemberFlags encoded via MemberFlags::to_acl_flags_bytes.
    let args = CreateEphemeralAtaPermission::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, permission_info, payer_info, system_program, permission_program, ..] =
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

    if permission_info.lamports() > 0 {
        return Ok(());
    }

    let members = [Member {
        flags: MemberFlags::from_acl_flags_bytes(args.flags_bytes()),
        #[allow(clippy::clone_on_copy)]
        pubkey: ephemeral_ata.owner.clone(),
    }];
    let members_args = MembersArgs {
        members: Some(&members),
    };

    CreatePermissionCpiBuilder::new(
        ephemeral_ata_info,
        permission_info,
        payer_info,
        system_program,
        &PERMISSION_PROGRAM_ID,
    )
    .members(members_args)
    .seeds(&[ephemeral_ata.owner.as_ref(), ephemeral_ata.mint.as_ref()])
    .bump(args.bump())
    .invoke()
}

pub struct CreateEphemeralAtaPermission<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl CreateEphemeralAtaPermission<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<CreateEphemeralAtaPermission, ProgramError> {
        if bytes.len() < 6 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(CreateEphemeralAtaPermission {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }

    #[inline]
    pub fn flags_bytes(&self) -> [u8; 5] {
        let mut bytes = [0u8; 5];
        unsafe {
            let slice = core::slice::from_raw_parts(self.raw.add(1), 5);
            bytes.copy_from_slice(slice);
        }
        bytes
    }
}
