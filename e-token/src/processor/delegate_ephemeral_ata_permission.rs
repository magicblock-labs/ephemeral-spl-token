use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::DelegatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account,
};
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};

#[inline(always)]
pub fn process_delegate_ephemeral_ata_permission(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer (also authority)
    // 1. [writable] Ephemeral ATA account (PDA derived from [owner, mint]) - signer via seeds
    // 2. []         Permission program (ACL)
    // 3. [writable] Permission PDA (derived from ["permission:", ephemeral_ata])
    // 4. []         System program
    // 5. [writable] Delegation buffer PDA (derived from [permission, permission_program])
    // 6. [writable] Delegation record PDA
    // 7. [writable] Delegation metadata PDA
    // 8. []         Delegation program
    // 9. []         Validator

    let args = DelegatePermissionArgs::try_from_bytes(instruction_data)?;

    let [payer_info, ephemeral_ata_info, permission_program, permission_info, system_program, delegation_buffer, delegation_record, delegation_metadata, delegation_program, validator, ..] =
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

    let expected_permission =
        permission_pda_from_permissioned_account(ephemeral_ata_info.address());
    if expected_permission != *permission_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump = [args.bump()];
    let seeds = [
        Seed::from(ephemeral_ata.owner.as_ref()),
        Seed::from(ephemeral_ata.mint.as_ref()),
        Seed::from(&bump),
    ];
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

pub struct DelegatePermissionArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl DelegatePermissionArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegatePermissionArgs, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(DelegatePermissionArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }
}
