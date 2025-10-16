use core::marker::PhantomData;
use ephemeral_spl_api::error::EphemeralSplError;
use ephemeral_spl_api::state::{Initializable, RawType};
use pinocchio::instruction::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::CreateAccount;
use {
    ephemeral_spl_api::state::global_vault::GlobalVault,
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult},
};

#[inline(always)]
pub fn process_initialize_global_vault(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Global Vault account (PDA derived from [mint])
    // 1. [signer]   Payer (funds the account creation)
    // 2. []         Mint  (seed)
    // 3. []         System program

    let args = InitializeGlobalVault::try_from_bytes(instruction_data)?;

    let [vault_info, payer_info, mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let bump = [args.bump()];
    let seed = [Seed::from(mint_info.key().as_slice()), Seed::from(&bump)];
    let signer_seeds = Signer::from(&seed);

    CreateAccount {
        from: payer_info,
        to: vault_info,
        space: GlobalVault::LEN as u64,
        lamports: Rent::get()?.minimum_balance(GlobalVault::LEN),
        owner: &ephemeral_spl_api::program::ID,
    }
    .invoke_signed(&[signer_seeds])?;

    // Ensure account data has the expected size
    let vault =
        unsafe { load_mut_unchecked::<GlobalVault>(vault_info.borrow_mut_data_unchecked())? };

    // Ensure the vault is not already initialized
    if vault.is_initialized() {
        return Err(EphemeralSplError::AlreadyInUse.into());
    }

    // Initialize the vault
    vault.mint = *mint_info.key();

    Ok(())
}

/// Instruction data for the `InitializeGlobalVault` instruction.
pub struct InitializeGlobalVault<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeGlobalVault<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeGlobalVault, ProgramError> {
        if bytes.len() < 1 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeGlobalVault {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw }
    }
}
