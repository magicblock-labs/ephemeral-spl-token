use ephemeral_rollups_pinocchio::instruction::{commit_accounts, DelegateAccountCpiBuilder};
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::fees_pda::{FeesPda, FEES_PDA_SEED, FEES_PDA_TAG};
use ephemeral_spl_api::state::{load_mut_unchecked, load_unchecked, Initializable, RawType};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

#[inline(always)]
pub fn process_initialize_fees_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Validator-scoped FEES PDA derived from ["FEES", validator]
    // 2. []         Validator account (seed)
    // 3. []         System program
    let [payer_info, fees_pda_info, validator_info, _system_program_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let (derived_fees_pda, bump) = derive_fees_pda(validator_info);
    if derived_fees_pda != *fees_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if is_valid_initialized_fees_pda(fees_pda_info, validator_info, bump) {
        return Ok(());
    }

    if fees_pda_info.lamports() > 0 {
        return Err(ProgramError::InvalidAccountData);
    }
    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let bump_seed = [bump];
    let signer_seeds = [
        Seed::from(FEES_PDA_SEED),
        Seed::from(validator_info.address().as_ref()),
        Seed::from(&bump_seed),
    ];
    let signer = Signer::from(&signer_seeds);

    CreateAccount {
        from: payer_info,
        to: fees_pda_info,
        space: FeesPda::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(FeesPda::LEN)?,
        owner: &ephemeral_spl_api::ID,
    }
    .invoke_signed(&[signer])?;

    let fees_pda = unsafe { load_mut_unchecked::<FeesPda>(fees_pda_info.borrow_unchecked_mut())? };
    fees_pda.tag = FEES_PDA_TAG;
    fees_pda.validator = *validator_info.address();
    fees_pda.bump = bump;

    Ok(())
}

#[inline(always)]
pub fn process_delegate_fees_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Validator-scoped FEES PDA derived from ["FEES", validator]
    // 2. []         Validator account (seed + delegated validator)
    // 3. []         Owner program (this program)
    // 4. [writable] Buffer account
    // 5. [writable] Delegation record account
    // 6. [writable] Delegation metadata account
    // 7. []         Delegation program
    // 8. []         System program
    let [payer_info, fees_pda_info, validator_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let program_id = ephemeral_spl_api::ID;
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    let (derived_fees_pda, bump) = derive_fees_pda(validator_info);
    if derived_fees_pda != *fees_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if !fees_pda_info.owned_by(&program_id) && !fees_pda_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }
    validate_fees_pda(fees_pda_info, validator_info, bump)?;

    if fees_pda_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if owner_program.address() != &program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if system_program.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let config = DelegateConfig {
        validator: Some(*validator_info.address()),
        ..DelegateConfig::default()
    };

    DelegateAccountCpiBuilder::new(
        payer_info,
        fees_pda_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(&[FEES_PDA_SEED, validator_info.address().as_ref()])
    .bump(bump)
    .config(config)
    .invoke_with_any_validator()
}

#[inline(always)]
pub fn process_commit_fees_pda(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Validator-scoped FEES PDA derived from ["FEES", validator]
    // 2. []         Validator account (seed)
    // 3. [writable] Magic context
    // 4. []         Magic program
    let [payer_info, fees_pda_info, validator_info, magic_context, magic_program, ..] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let program_id = ephemeral_spl_api::ID;
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    let (derived_fees_pda, bump) = derive_fees_pda(validator_info);
    if derived_fees_pda != *fees_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !fees_pda_info.owned_by(&program_id) && !fees_pda_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }

    validate_fees_pda(fees_pda_info, validator_info, bump)?;
    commit_accounts(
        payer_info,
        &[fees_pda_info.clone()],
        magic_context,
        magic_program,
        None,
        None,
    )
}

#[inline(always)]
fn derive_fees_pda(validator_info: &AccountView) -> (ephemeral_spl_api::Address, u8) {
    ephemeral_spl_api::Address::find_program_address(
        &[FEES_PDA_SEED, validator_info.address().as_ref()],
        &ephemeral_spl_api::ID,
    )
}

#[inline(always)]
fn is_valid_initialized_fees_pda(
    fees_pda_info: &AccountView,
    validator_info: &AccountView,
    bump: u8,
) -> bool {
    let program_id = ephemeral_spl_api::ID;
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if !fees_pda_info.owned_by(&program_id) && !fees_pda_info.owned_by(&delegation_program) {
        return false;
    }

    let fees_pda = match unsafe { load_unchecked::<FeesPda>(fees_pda_info.borrow_unchecked()) } {
        Ok(fees_pda) => fees_pda,
        Err(_) => return false,
    };

    fees_pda.is_initialized()
        && fees_pda.validator == *validator_info.address()
        && fees_pda.bump == bump
}

#[inline(always)]
fn validate_fees_pda(
    fees_pda_info: &AccountView,
    validator_info: &AccountView,
    bump: u8,
) -> ProgramResult {
    let fees_pda = unsafe { load_unchecked::<FeesPda>(fees_pda_info.borrow_unchecked())? };
    if !fees_pda.is_initialized()
        || fees_pda.validator != *validator_info.address()
        || fees_pda.bump != bump
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}
