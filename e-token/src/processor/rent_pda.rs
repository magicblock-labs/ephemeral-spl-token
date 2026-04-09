use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::Address;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

pub const RENT_PDA_SEED: &[u8] = b"rent";
const RENT_PDA_AND_BUMP: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[RENT_PDA_SEED], &crate::ID.as_array());
pub const RENT_PDA: Address = Address::new_from_array(RENT_PDA_AND_BUMP.0);
pub const RENT_PDA_BUMP: u8 = RENT_PDA_AND_BUMP.1;

#[inline(always)]
pub fn process_initialize_rent_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Global rent PDA derived from ["rent"]
    // 2. []         System program
    let [payer_info, rent_pda_info, _system_program_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let required_lamports = Rent::get()?.try_minimum_balance(0)?;
    if &RENT_PDA != rent_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if is_valid_initialized_rent_pda(rent_pda_info, required_lamports) {
        return Ok(());
    }

    if rent_pda_info.lamports() > 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let bump_seed = [RENT_PDA_BUMP];
    let signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&bump_seed)];
    let signer = Signer::from(&signer_seeds);

    CreateAccount {
        from: payer_info,
        to: rent_pda_info,
        space: 0,
        lamports: required_lamports,
        owner: &pinocchio_system::ID,
    }
    .invoke_signed(&[signer])?;

    Ok(())
}

#[inline(always)]
fn is_valid_initialized_rent_pda(rent_pda_info: &AccountView, required_lamports: u64) -> bool {
    rent_pda_info.owned_by(&pinocchio_system::ID)
        && rent_pda_info.data_len() == 0
        && rent_pda_info.lamports() >= required_lamports
}
