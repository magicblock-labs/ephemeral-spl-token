use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, TransferQueue, QUEUE_SEED};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator]).
///  2: []                  - SPL     : Mint account.
///  3: []                  - Program : Owner program (this program).
///  4: [writable]          - PDA     : Buffer account.
///  5: [writable]          - PDA     : Delegation record account.
///  6: [writable]          - PDA     : Delegation metadata account.
///  7: []                  - Program : Delegation program.
///  8: []                  - Builtin : System program.
///
/// Instruction Data: None
///
pub fn process_delegate_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        queue_info,
        mint_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
    ] = require_n_accounts!(accounts, 9);

    require!(
        instruction_data.is_empty(),
        ProgramError::InvalidInstructionData
    );

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    require!(
        queue_info.owned_by(&crate::ID) || queue_info.owned_by(&delegation_program),
        ProgramError::IllegalOwner
    );

    let (bump, validator) = {
        let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
        require_eq_keys!(
            &header.mint,
            mint_info.address(),
            ProgramError::InvalidAccountData
        );

        let bump = header.bump;
        let derived_queue =
            TransferQueue::derive_pda(mint_info.address(), &header.validator, bump)?;
        require_eq_keys!(
            &derived_queue,
            queue_info.address(),
            ProgramError::InvalidSeeds
        );

        (bump, header.validator)
    };

    if queue_info.owned_by(&delegation_program) {
        return Ok(());
    }

    require_eq_keys!(
        owner_program.address(),
        &crate::ID,
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(
        system_program.address(),
        &pinocchio_system::ID,
        ProgramError::IncorrectProgramId
    );

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
