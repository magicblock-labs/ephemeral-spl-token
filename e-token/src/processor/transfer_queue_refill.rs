use core::mem::size_of;

use ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer;
use ephemeral_spl_api::state::{
    transfer_queue::{queue_views_checked, QUEUE_SEED},
    transfer_queue_refill::{
        derive_transfer_queue_refill_state_address, TransferQueueRefillState,
        QUEUE_REFILL_STATE_SEED,
    },
};
use ephemeral_spl_api::{
    consts::TRANSFER_QUEUE_REFILL_LAMPORTS, instruction::SPONSORED_LAMPORTS_TRANSFER,
};
use pinocchio::cpi::{invoke_signed_with_bounds, Seed, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::sysvars::{rent::Rent, Sysvar};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

use crate::{
    assert_owner, assert_signer,
    processor::{
        lamports_pda::derive_lamports_pda,
        rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    },
};

pub(crate) const MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX: u8 = 1;
// This path may need to create the refill-state PDA on first use, so it needs
// more headroom than a pure flag update.
pub(crate) const MARK_TRANSFER_QUEUE_REFILL_PENDING_COMPUTE_UNITS: u32 = 50_000;

const SPONSORED_LAMPORTS_TRANSFER_CPI_ACCOUNTS: usize = 11;
const SPONSORED_LAMPORTS_TRANSFER_DATA_LEN: usize = 1 + 8 + 32;

#[inline(always)]
pub(crate) fn refill_transfer_queue_amounts(
    queue_data_len: usize,
) -> Result<(u64, u64), ProgramError> {
    let queue_rent_exemption = Rent::get()?.try_minimum_balance(queue_data_len)?;
    Ok((queue_rent_exemption, TRANSFER_QUEUE_REFILL_LAMPORTS))
}

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
    if source_program.address() != &ephemeral_spl_api::program::id_address() {
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

#[inline(never)]
pub fn process_pending_transfer_queue_refill(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [refill_state_info, rest @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Exit early if refill_state_info does not exist (refill was not requested)
    if refill_state_info.lamports() == 0
        || !refill_state_info.owned_by(&ephemeral_spl_api::program::id_address())
    {
        return Ok(());
    }

    let [queue_info, rent_pda_info, lamports_pda_info, owner_program_info, buffer_acc, delegation_record, delegation_metadata, delegation_program_info, system_program_info, queue_delegation_record_info, ..] =
        rest
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let program_id = ephemeral_spl_api::program::id_address();
    if refill_state_info.lamports() == 0 || !refill_state_info.owned_by(&program_id) {
        return Ok(());
    }

    validate_queue_refill_state_address(refill_state_info, queue_info.address())?;
    validate_rent_pda(rent_pda_info)?;

    let (_, refill_lamports) = refill_transfer_queue_amounts(queue_info.data_len())?;
    let (refill_lamports_pda, _, refill_salt) = queue_refill_lamports_pda(queue_info.address());
    if refill_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if owner_program_info.address() != &program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    trigger_queue_refill_via_sponsored_transfer(
        owner_program_info,
        rent_pda_info,
        lamports_pda_info,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        delegation_program_info,
        system_program_info,
        queue_info,
        queue_delegation_record_info,
        refill_lamports,
        &refill_salt,
    )?;

    close_program_account_to_recipient(refill_state_info, rent_pda_info)
}

#[inline(always)]
pub(crate) fn queue_refill_state_address(queue: &Address) -> Address {
    derive_transfer_queue_refill_state_address(queue).0
}

#[inline(always)]
fn parse_escrow_index(instruction_data: &[u8]) -> Result<u8, ProgramError> {
    if instruction_data.len() != 1 {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(instruction_data[0])
}

fn validate_queue_account(queue_info: &AccountView) -> Result<(), ProgramError> {
    let program_id = ephemeral_spl_api::program::id_address();
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if !queue_info.owned_by(&program_id) && !queue_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }

    let queue_data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(queue_data)?;
    let bump_seed = [header.bump];
    let derived_queue = Address::create_program_address(
        &[
            QUEUE_SEED,
            header.mint.as_ref(),
            header.validator.as_ref(),
            bump_seed.as_ref(),
        ],
        &program_id,
    )
    .map_err(|_| ProgramError::InvalidAccountData)?;
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(())
}

fn validate_rent_pda(rent_pda_info: &AccountView) -> Result<(), ProgramError> {
    assert_owner!(rent_pda_info, &pinocchio_system::ID);
    if RENT_PDA != *rent_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if rent_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

fn validate_queue_refill_state_address(
    refill_state_info: &AccountView,
    queue: &Address,
) -> Result<(Address, u8), ProgramError> {
    let (expected_refill_state, bump) = derive_transfer_queue_refill_state_address(queue);
    if expected_refill_state != *refill_state_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok((expected_refill_state, bump))
}

#[inline(always)]
fn queue_refill_lamports_pda(queue: &Address) -> (Address, u8, [u8; 32]) {
    let mut salt = [0_u8; 32];
    salt.copy_from_slice(queue.as_ref());
    let (lamports_pda, bump) = derive_lamports_pda(&RENT_PDA, queue, &salt);
    (lamports_pda, bump, salt)
}

#[allow(clippy::too_many_arguments)]
fn trigger_queue_refill_via_sponsored_transfer(
    owner_program_info: &AccountView,
    rent_pda_info: &AccountView,
    lamports_pda_info: &AccountView,
    buffer_acc: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
    delegation_program_info: &AccountView,
    system_program_info: &AccountView,
    queue_info: &AccountView,
    queue_delegation_record_info: &AccountView,
    refill_lamports: u64,
    salt: &[u8; 32],
) -> ProgramResult {
    let mut sponsored_transfer_data = [0_u8; SPONSORED_LAMPORTS_TRANSFER_DATA_LEN];
    sponsored_transfer_data[0] = SPONSORED_LAMPORTS_TRANSFER;
    sponsored_transfer_data[1..9].copy_from_slice(&refill_lamports.to_le_bytes());
    sponsored_transfer_data[9..].copy_from_slice(salt);

    let sponsored_transfer_accounts = [
        InstructionAccount::readonly_signer(rent_pda_info.address()),
        InstructionAccount::writable(rent_pda_info.address()),
        InstructionAccount::writable(lamports_pda_info.address()),
        InstructionAccount::readonly(owner_program_info.address()),
        InstructionAccount::writable(buffer_acc.address()),
        InstructionAccount::writable(delegation_record.address()),
        InstructionAccount::writable(delegation_metadata.address()),
        InstructionAccount::readonly(delegation_program_info.address()),
        InstructionAccount::readonly(system_program_info.address()),
        InstructionAccount::writable(queue_info.address()),
        InstructionAccount::readonly(queue_delegation_record_info.address()),
    ];
    let sponsored_transfer_instruction = InstructionView {
        program_id: owner_program_info.address(),
        accounts: &sponsored_transfer_accounts,
        data: &sponsored_transfer_data,
    };
    let sponsored_transfer_account_refs = [
        rent_pda_info,
        rent_pda_info,
        lamports_pda_info,
        owner_program_info,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        delegation_program_info,
        system_program_info,
        queue_info,
        queue_delegation_record_info,
    ];
    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seeds);

    invoke_signed_with_bounds::<SPONSORED_LAMPORTS_TRANSFER_CPI_ACCOUNTS>(
        &sponsored_transfer_instruction,
        &sponsored_transfer_account_refs,
        &[rent_signer.clone()],
    )
}

fn ensure_queue_refill_state_exists(
    rent_pda_info: &AccountView,
    refill_state_info: &AccountView,
    queue: &Address,
    refill_state_bump: u8,
) -> ProgramResult {
    validate_rent_pda(rent_pda_info)?;

    let program_id = ephemeral_spl_api::program::id_address();
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
            owner: &program_id,
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
            owner: &program_id,
        }
        .invoke_signed(&[refill_state_signer])?;
    }

    assert_owner!(refill_state_info, &program_id);
    let refill_state_data = unsafe { refill_state_info.borrow_unchecked_mut() };
    for b in refill_state_data.iter_mut() {
        *b = 0;
    }
    Ok(())
}

fn close_program_account_to_recipient(
    account: &AccountView,
    recipient: &AccountView,
) -> ProgramResult {
    if *recipient.address() == *account.address() {
        return Err(ProgramError::InvalidArgument);
    }

    let lamports_to_refund = account.lamports();
    let updated_recipient_lamports = recipient
        .lamports()
        .checked_add(lamports_to_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    recipient.set_lamports(updated_recipient_lamports);
    account.set_lamports(0);
    account.close()?;
    Ok(())
}
