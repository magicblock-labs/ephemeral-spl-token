use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::ClosePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

/// Close the permission PDA associated with an Ephemeral ATA.
///
/// Expected accounts:
/// 0. [signer]   Payer (authority)
/// 1. [writable] Ephemeral ATA account (permissioned account)
/// 2. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
/// 3. []         Permission program (ACL)
pub fn process_close_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = CloseEphemeralAtaPermission::try_from_bytes(instruction_data)?;

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
        return Err(ProgramError::InvalidAccountData);
    }

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    if expected_permission != *permission_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    ClosePermissionCpiBuilder::new(
        payer_info,
        payer_info, // ephemeral_ata_info
        ephemeral_ata_info,
        permission_info,
        &PERMISSION_PROGRAM_ID,
    )
    .seeds(&[ephemeral_ata.owner.as_ref(), ephemeral_ata.mint.as_ref()])
    .bump(args.bump())
    .invoke()
}

pub struct CloseEphemeralAtaPermission<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl CloseEphemeralAtaPermission<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<CloseEphemeralAtaPermission, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(CloseEphemeralAtaPermission {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }
}
