use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};

use crate::processor::internal::rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Global rent PDA derived from ["rent"].
///  2: []                  - Builtin : System program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_initialize_rent_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        _system_program_info,
    ] = require_n_accounts!(accounts, 3);

    require!(
        instruction_data.is_empty(),
        ProgramError::InvalidInstructionData
    );

    let required_lamports = Rent::get()?.try_minimum_balance(0)?;
    require_eq_keys!(
        &RENT_PDA,
        rent_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    if is_valid_initialized_rent_pda(rent_pda_info, required_lamports) {
        return Ok(());
    }

    require!(
        rent_pda_info.lamports() == 0,
        ProgramError::InvalidAccountData
    );
    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

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
    .invoke_signed(&[signer])
}

#[inline(always)]
fn is_valid_initialized_rent_pda(rent_pda_info: &AccountView, required_lamports: u64) -> bool {
    rent_pda_info.owned_by(&pinocchio_system::ID)
        && rent_pda_info.data_len() == 0
        && rent_pda_info.lamports() >= required_lamports
}
