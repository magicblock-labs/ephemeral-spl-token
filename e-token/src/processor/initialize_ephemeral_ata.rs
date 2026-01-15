use core::marker::PhantomData;
use ephemeral_spl_api::state::RawType;
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::CreateAccount;
use {
    ephemeral_spl_api::state::ephemeral_ata::EphemeralAta,
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
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
            .eq(&ephemeral_spl_api::program::id_address())
        {
            return Ok(());
        }
    }

    let bump = [args.bump()];
    let seed = [
        Seed::from(user_info.address().as_ref()),
        Seed::from(mint_info.address().as_ref()),
        Seed::from(&bump),
    ];
    let signer_seeds = Signer::from(&seed);

    CreateAccount {
        from: payer_info,
        to: ephemeral_ata_info,
        space: EphemeralAta::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?,
        owner: &ephemeral_spl_api::program::id_address(),
    }
    .invoke_signed(&[signer_seeds])?;

    // Ensure account data has the expected size
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };

    // Initialize the ephemeral ATA
    // Set the owner to the provided user; payer only funds account creation
    ephemeral_ata.owner = user_info.address().clone();
    ephemeral_ata.mint = mint_info.address().clone();
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
