use crate::processor::common;
use core::marker::PhantomData;

use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};

#[inline(always)]
pub fn process_initialize_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [user, mint])
    // 1. []         Payer (funding account)
    // 2. []         User  (seed)
    // 3. []         Mint  (seed)

    let args = InitializeEphemeralAta::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, payer_info, user_info, mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    unsafe {
        // Make init idempotent
        if ephemeral_ata_info
            .owner()
            .eq(&ephemeral_spl_api::program::ID)
        {
            return Ok(());
        }
    }

    common::initialize_ephemeral_ata(
        &Rent::get()?,
        payer_info,
        user_info,
        mint_info,
        ephemeral_ata_info,
        args.bump(),
    )?;

    Ok(())
}

/// Instruction data for the `InitializeMint` instruction.
pub struct InitializeEphemeralAta<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeEphemeralAta<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeEphemeralAta, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeEphemeralAta {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }
}
