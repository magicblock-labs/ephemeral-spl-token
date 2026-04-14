use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, TransferQueue, QUEUE_SEED};
use pinocchio::{address::address_eq, error::ProgramError, AccountView, ProgramResult};

use crate::assert_signer;

pub fn process_delegate_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint, validator]
    // 2. []         Mint account
    // 3. []         Owner program (this program)
    // 4. [writable] Buffer account
    // 5. [writable] Delegation record account
    // 6. [writable] Delegation metadata account
    // 7. []         Delegation program
    // 8. []         System program
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    let [payer_info, queue_info, mint_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if !queue_info.owned_by(&crate::ID) && !queue_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }

    let (bump, validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        if !address_eq(&header.mint, mint_info.address()) {
            return Err(ProgramError::InvalidAccountData);
        }

        let bump = header.bump;
        let derived_queue =
            TransferQueue::derive_pda(mint_info.address(), &header.validator, bump)?;
        if !address_eq(&derived_queue, queue_info.address()) {
            return Err(ProgramError::InvalidSeeds);
        }

        (bump, header.validator)
    };

    if queue_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if !address_eq(owner_program.address(), &crate::ID) {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !address_eq(system_program.address(), &pinocchio_system::ID) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let config = DelegateConfig {
        validator: Some(validator),
        ..DelegateConfig::default()
    };
    let seeds: &[&[u8]] = &[QUEUE_SEED, mint_info.address().as_ref(), validator.as_ref()];

    DelegateAccountCpiBuilder::new(
        payer_info,
        queue_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(bump)
    .config(config)
    .invoke_with_any_validator()
}
