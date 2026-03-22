use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, QUEUE_SEED};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

pub fn process_delegate_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint]
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

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let program_id = ephemeral_spl_api::ID;
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if !queue_info.owned_by(&program_id) && !queue_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }

    let bump = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        if header.mint != *mint_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }

        let bump = header.bump;
        let bump_seed = [bump];
        let derived_queue = ephemeral_spl_api::Address::create_program_address(
            &[QUEUE_SEED, mint_info.address().as_ref(), bump_seed.as_ref()],
            &program_id,
        )
        .map_err(|_| ProgramError::InvalidAccountData)?;
        if derived_queue != *queue_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        bump
    };

    if queue_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if owner_program.address() != &program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if system_program.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let config = DelegateConfig {
        validator: Some(pinocchio_system::ID),
        ..DelegateConfig::default()
    };
    let seeds: &[&[u8]] = &[QUEUE_SEED, mint_info.address().as_ref()];

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
