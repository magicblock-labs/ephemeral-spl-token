use core::mem::size_of;

use ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer;
use ephemeral_spl_api::state::transfer_queue_refill::{
    TransferQueueRefillState, QUEUE_REFILL_STATE_SEED,
};
use pinocchio::sysvars::{rent::Rent, Sysvar};
use pinocchio::{
    address::address_eq,
    cpi::{Seed, Signer},
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

use crate::{
    assert_owner, assert_signer,
    processor::{
        initialize_rent_pda::{RENT_PDA_BUMP, RENT_PDA_SEED},
        internal::transfer_queue_refill::{
            validate_queue_account, validate_queue_refill_state_address, validate_rent_pda,
        },
    },
};

#[inline(never)]
pub fn process_mark_transfer_queue_refill_pending(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;
    let [rent_pda_info, refill_state_info, system_program_info, source_program, escrow_authority, escrow_signer] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if system_program_info.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !address_eq(source_program.address(), &crate::ID) {
        return Err(ProgramError::IncorrectAuthority);
    }
    assert_signer!(escrow_signer);

    let expected_escrow =
        ephemeral_balance_pda_from_payer(escrow_authority.address(), escrow_index);
    if expected_escrow != *escrow_signer.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    validate_queue_account(escrow_authority)?;
    let (_, refill_state_bump) =
        validate_queue_refill_state_address(refill_state_info, escrow_authority.address())?;
    ensure_queue_refill_state_exists(
        rent_pda_info,
        refill_state_info,
        escrow_authority.address(),
        refill_state_bump,
    )
}

#[inline(always)]
fn parse_escrow_index(instruction_data: &[u8]) -> Result<u8, ProgramError> {
    if instruction_data.len() != 1 {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(instruction_data[0])
}

fn ensure_queue_refill_state_exists(
    rent_pda_info: &AccountView,
    refill_state_info: &AccountView,
    queue: &Address,
    refill_state_bump: u8,
) -> ProgramResult {
    validate_rent_pda(rent_pda_info)?;

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seeds);

    let refill_state_bump_seed = [refill_state_bump];
    let refill_state_signer_seeds = [
        Seed::from(QUEUE_REFILL_STATE_SEED),
        Seed::from(queue.as_ref()),
        Seed::from(&refill_state_bump_seed),
    ];
    let refill_state_signer = Signer::from(&refill_state_signer_seeds);

    if refill_state_info.lamports() == 0 {
        CreateAccount {
            from: rent_pda_info,
            to: refill_state_info,
            space: size_of::<TransferQueueRefillState>() as u64,
            lamports: Rent::get()?.try_minimum_balance(size_of::<TransferQueueRefillState>())?,
            owner: &crate::ID,
        }
        .invoke_signed(&[rent_signer, refill_state_signer])?;
    } else if refill_state_info.owned_by(&pinocchio_system::ID) {
        let refill_state_size = size_of::<TransferQueueRefillState>();
        let rent_exempt_balance = Rent::get()?
            .try_minimum_balance(refill_state_size)?
            .saturating_sub(refill_state_info.lamports());
        if rent_exempt_balance > 0 {
            Transfer {
                from: rent_pda_info,
                to: refill_state_info,
                lamports: rent_exempt_balance,
            }
            .invoke_signed(&[rent_signer.clone()])?;
        }

        Allocate {
            account: refill_state_info,
            space: refill_state_size as u64,
        }
        .invoke_signed(&[refill_state_signer.clone()])?;

        Assign {
            account: refill_state_info,
            owner: &crate::ID,
        }
        .invoke_signed(&[refill_state_signer])?;
    }

    assert_owner!(refill_state_info, &crate::ID);
    Ok(())
}
