use ephemeral_spl_api::state::{Initializable, RawType};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::CreateAccount;
use {
    ephemeral_spl_api::state::ephemeral_ata::EphemeralAta,
    ephemeral_spl_api::state::{load_mut_unchecked, load_unchecked},
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

    // Validate PDA derivation up front, even for idempotent re-initialization.
    let (derived_pda, _) = ephemeral_spl_api::Address::find_program_address(
        &[user_info.address().as_ref(), mint_info.address().as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_pda != *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    // Make init idempotent even if the account is currently delegated (owner changed).
    if let Ok(ephemeral_ata) =
        unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked()) }
    {
        if ephemeral_ata.is_initialized()
            && ephemeral_ata.owner == *user_info.address()
            && ephemeral_ata.mint == *mint_info.address()
        {
            return Ok(());
        }
    }

    // Any other pre-existing account at this PDA is invalid for initialization.
    if ephemeral_ata_info.lamports() > 0 {
        return Err(ProgramError::InvalidAccountData);
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
    ephemeral_ata.owner = *user_info.address();
    ephemeral_ata.mint = *mint_info.address();
    ephemeral_ata.amount = 0;

    Ok(())
}

/// Instruction data for the `InitializeMint` instruction.
pub struct InitializeEphemeralAta {
    bump: u8,
}

impl InitializeEphemeralAta {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeEphemeralAta, ProgramError> {
        if bytes.len() != 1 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeEphemeralAta { bump: bytes[0] })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        self.bump
    }
}
