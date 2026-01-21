#![no_std]

use pinocchio::{
    cpi::{Seed, Signer},
    entrypoint::deserialize,
    error::ProgramError,
    no_allocator,
    nostd_panic_handler,
    AccountView, Address, ProgramResult, MAX_TX_ACCOUNTS, SUCCESS,
};
use pinocchio_pubkey::pubkey;
use pinocchio_system::instructions::CreateAccount;
use pinocchio::sysvars::{rent::Rent, Sysvar};

no_allocator!();
nostd_panic_handler!();

const PERMISSION_PROGRAM_ID: Address =
    Address::new_from_array(pubkey!("ACLseoPoyC3cBqoUtkbjZ4aDrkurZW86v19pXz2XQnp1"));

#[no_mangle]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    const UNINIT: core::mem::MaybeUninit<AccountView> =
        core::mem::MaybeUninit::<AccountView>::uninit();
    let mut accounts = [UNINIT; { MAX_TX_ACCOUNTS }];

    let (_, count, instruction_data) = deserialize::<MAX_TX_ACCOUNTS>(input, &mut accounts);

    match process_instruction(
        core::slice::from_raw_parts(accounts.as_ptr() as _, count),
        instruction_data,
    ) {
        Ok(()) => SUCCESS,
        Err(error) => error.into(),
    }
}

fn process_instruction(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    let [permissioned_account, permission_account, payer, system_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    if permission_account.lamports() > 0 {
        return Ok(());
    }

    let (expected, bump) = Address::find_program_address(
        &[b"permission:", permissioned_account.address().as_ref()],
        &PERMISSION_PROGRAM_ID,
    );
    if expected != *permission_account.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump_seed = [bump];
    let seeds = [
        Seed::from(b"permission:"),
        Seed::from(permissioned_account.address().as_ref()),
        Seed::from(&bump_seed),
    ];
    let signer_seeds = Signer::from(&seeds);

    CreateAccount {
        from: payer,
        to: permission_account,
        space: 0,
        lamports: Rent::get()?.try_minimum_balance(0)?,
        owner: &PERMISSION_PROGRAM_ID,
    }
    .invoke_signed(&[signer_seeds])?;

    let _ = system_program;

    Ok(())
}
