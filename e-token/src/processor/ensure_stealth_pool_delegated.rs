use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::stealth_pool::StealthPool;
use ephemeral_spl_api::state::RawType;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

///
/// Executes on: base.
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Stealth pool account.
///  2: []                  - Program : Owner program (this program).
///  3: [writable]          - PDA     : Buffer account.
///  4: [writable]          - PDA     : Delegation record account.
///  5: [writable]          - PDA     : Delegation metadata account.
///  6: []                  - Program : Delegation program.
///  7: []                  - Builtin : System program.
///
/// Instruction Data: EnsureStealthPoolDelegatedArgs
///
pub fn process_ensure_stealth_pool_delegated(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        stealth_pool_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
    ] = require_n_accounts!(accounts, 8);

    let args = EnsureStealthPoolDelegatedArgs::decode(instruction_data)?;

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    let (derived_pool, bump) = StealthPool::find_pda(args.handle_hash());
    require_eq_keys!(
        &derived_pool,
        stealth_pool_info.address(),
        ProgramError::InvalidSeeds
    );

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if stealth_pool_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if !stealth_pool_info.owned_by(&crate::ID) {
        require!(
            stealth_pool_info.lamports() == 0,
            ProgramError::IllegalOwner
        );

        let rent = Rent::get()?;
        let lamports = rent.try_minimum_balance(StealthPool::LEN)?;
        let bump_seed = [bump];
        let signer_seeds = StealthPool::signer_seeds(args.handle_hash(), &bump_seed);
        let signer = Signer::from(&signer_seeds);

        CreateAccount {
            from: payer_info,
            to: stealth_pool_info,
            space: StealthPool::LEN as u64,
            lamports,
            owner: &crate::ID,
        }
        .invoke_signed(&[signer])?;
    } else {
        require!(
            stealth_pool_info.data_len() == StealthPool::LEN,
            ProgramError::InvalidAccountData
        );
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
        validator: args
            .validator()
            .map(|slice| Address::new_from_array(*slice)),
        ..DelegateConfig::default()
    };
    let seeds = StealthPool::seeds(args.handle_hash());

    DelegateAccountCpiBuilder::new(
        payer_info,
        stealth_pool_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(&seeds)
    .bump(bump)
    .config(config)
    .invoke()
}

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct EnsureStealthPoolDelegatedArgs {
    pub handle_hash: [u8; 32],
    pub validator: Option<[u8; 32]>,
}

static_assertions::const_assert!(matches!(
    EnsureStealthPoolDelegatedArgs::DATA_LENS,
    [32, 64]
));
