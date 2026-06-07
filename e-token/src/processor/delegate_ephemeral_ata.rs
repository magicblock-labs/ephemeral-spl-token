use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use dlp_api::{requires::require_initialized_delegation_record, state::DelegationRecord};
use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::require_n_accounts;
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use ephemeral_spl_api::{debug_log, error::EphemeralSplError};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

fn validate_existing_delegation(
    ephemeral_ata_info: &AccountView,
    delegation_record: &AccountView,
    requested_validator: Option<&[u8; 32]>,
) -> ProgramResult {
    let Some(requested_validator) = requested_validator else {
        return Ok(());
    };

    require_initialized_delegation_record(ephemeral_ata_info, delegation_record, true)?;

    let delegation_record_data = delegation_record.try_borrow()?;
    let delegation_record =
        DelegationRecord::try_from_bytes_with_discriminator(&delegation_record_data)
            .map_err(|_| ProgramError::InvalidAccountData)?;
    let current_validator = Address::new_from_array(delegation_record.authority.to_bytes());
    let requested_validator = Address::new_from_array(*requested_validator);

    if current_validator == requested_validator {
        return Ok(());
    }

    let current = current_validator.to_string();
    let requested = requested_validator.to_string();
    pinocchio_log::log!(
        "Ephemeral ATA is already delegated to validator {}, requested validator {}",
        current.as_str(),
        requested.as_str(),
    );

    Err(EphemeralSplError::EphemeralAtaValidatorMismatch.into())
}

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer (seed used to derive the Ephemeral ATA PDA).
///  1: [writable]          - PDA     : Ephemeral ATA account (PDA derived from [payer, mint]).
///  2: []                  - Program : Owner program.
///  3: [writable]          - PDA     : Buffer account.
///  4: [writable]          - PDA     : Delegation record account.
///  5: [writable]          - PDA     : Delegation metadata account.
///  6: []                  - Program : Delegation program.
///  7: []                  - Builtin : System program.
///
/// Instruction Data: DelegateArgs
///
pub fn process_delegate_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
    ] = require_n_accounts!(accounts, 8);

    let args = DelegateArgs::decode(instruction_data)?;

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        validate_existing_delegation(ephemeral_ata_info, delegation_record, args.validator())?;
        return Ok(());
    }

    // Load Ephemeral ATA account
    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

    let config = DelegateConfig {
        validator: args
            .validator()
            .map(|slice| Address::new_from_array(*slice)),
        ..DelegateConfig::default()
    };

    #[allow(clippy::clone_on_copy)]
    let mint = ephemeral_ata.mint.clone();
    #[allow(clippy::clone_on_copy)]
    let owner = ephemeral_ata.owner.clone();
    let seeds: &[&[u8]] = &[owner.as_ref(), mint.as_ref()];

    debug_log!("Delegating eata");

    DelegateAccountCpiBuilder::new(
        payer_info,
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(ephemeral_ata.bump)
    .config(config)
    .invoke()
}

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DelegateArgs {
    pub validator: Option<[u8; 32]>,
}

static_assertions::const_assert!(matches!(DelegateArgs::DATA_LENS, [0, 32]));
