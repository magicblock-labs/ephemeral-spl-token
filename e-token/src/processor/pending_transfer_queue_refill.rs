use ephemeral_spl_api::instruction::SPONSORED_LAMPORTS_TRANSFER;
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::{
    address::address_eq,
    cpi::{invoke_signed_with_bounds, Seed, Signer},
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::{
    initialize_rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    internal::{
        lamports_pda::derive_lamports_pda,
        transfer_queue_refill::{
            refill_transfer_queue_amounts, validate_queue_refill_state_address, validate_rent_pda,
        },
    },
};

const SPONSORED_LAMPORTS_TRANSFER_CPI_ACCOUNTS: usize = 11;
const SPONSORED_LAMPORTS_TRANSFER_DATA_LEN: usize = 1 + 8 + 32;

#[inline(never)]
pub fn process_pending_transfer_queue_refill(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [refill_state_info, rest @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Exit early if refill_state_info does not exist (refill was not requested)
    if refill_state_info.lamports() == 0 || !refill_state_info.owned_by(&crate::ID) {
        return Ok(());
    }

    let [queue_info, rent_pda_info, lamports_pda_info, owner_program_info, buffer_acc, delegation_record, delegation_metadata, delegation_program_info, system_program_info, queue_delegation_record_info, ..] =
        rest
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    validate_queue_refill_state_address(refill_state_info, queue_info.address())?;
    validate_rent_pda(rent_pda_info)?;

    let (_, refill_lamports) = refill_transfer_queue_amounts(queue_info.data_len())?;
    let (refill_lamports_pda, _, refill_salt) = queue_refill_lamports_pda(queue_info.address());
    if refill_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !address_eq(owner_program_info.address(), &crate::ID) {
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
