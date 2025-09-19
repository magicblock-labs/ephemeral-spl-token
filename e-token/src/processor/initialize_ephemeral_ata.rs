use core::marker::PhantomData;
use pinocchio::instruction::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::CreateAccount;
use {
    ephemeral_spl_api::state::ephemeral_ata::EphemeralAta,
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult},
};
use ephemeral_spl_api::error::EphemeralSplError;
use ephemeral_spl_api::state::{Initializable, RawType};

#[inline(always)]
pub fn process_initialize_ephemeral_ata(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [payer, mint])
    // 1. []         Payer (seed)
    // 2. []         Mint  (seed)

    let args = InitializeEphemeralAta::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, payer_info, mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let bump = [args.bump()];
    let seed = [Seed::from(payer_info.key().as_slice()), Seed::from(mint_info.key().as_slice()), Seed::from(&bump)];
    let signer_seeds = Signer::from(&seed);

    CreateAccount {
        from: payer_info,
        to: ephemeral_ata_info,
        space: EphemeralAta::LEN as u64,
        lamports: Rent::get()?.minimum_balance(EphemeralAta::LEN),
        owner: &ephemeral_spl_api::program::ID,
    }.invoke_signed(&[signer_seeds])?;

    // Ensure account data has the expected size
    let ephemeral_ata = unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_mut_data_unchecked())? };

    // Ensure the ephemeral ATA is not already initialized
    if ephemeral_ata.is_initialized() {
        return Err(EphemeralSplError::AlreadyInUse.into());
    }

    // Initialize the ephemeral ATA
    ephemeral_ata.mint = *mint_info.key();
    ephemeral_ata.amount = 0;

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
        if bytes.len() < 1 {
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