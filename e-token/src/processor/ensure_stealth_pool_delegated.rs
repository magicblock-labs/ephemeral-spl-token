use ephemeral_rollups_pinocchio::{instruction::DelegateAccountCpiBuilder, types::DelegateConfig};
use ephemeral_spl_api::{
    instructions::EnsureStealthPoolDelegatedArgs,
    require, require_eq_keys, require_n_accounts,
    state::{stealth_pool::StealthPool, RawType},
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;
use wheels::layout::Decodable as _;

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
pub fn process_ensure_stealth_pool_delegated(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
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

    require!(payer_info.is_signer(), ProgramError::MissingRequiredSignature);

    let handle = StealthPool::handle_from_storage(args.handle())?;
    let (derived_pool, bump) = StealthPool::find_pda(handle)?;
    require_eq_keys!(&derived_pool, stealth_pool_info.address(), ProgramError::InvalidSeeds);

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if stealth_pool_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if !stealth_pool_info.owned_by(&crate::ID) {
        require!(stealth_pool_info.lamports() == 0, ProgramError::IllegalOwner);

        let rent = Rent::get()?;
        let lamports = rent.try_minimum_balance(StealthPool::LEN)?;
        let bump_seed = [bump];
        if handle.len() <= StealthPool::MAX_HANDLE_SEED_BYTES {
            let signer_seeds = [
                Seed::from(StealthPool::SEED),
                Seed::from(handle),
                Seed::from(&bump_seed),
            ];
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
            let signer_seeds = [
                Seed::from(StealthPool::SEED),
                Seed::from(&handle[..StealthPool::MAX_HANDLE_SEED_BYTES]),
                Seed::from(&handle[StealthPool::MAX_HANDLE_SEED_BYTES..]),
                Seed::from(&bump_seed),
            ];
            let signer = Signer::from(&signer_seeds);

            CreateAccount {
                from: payer_info,
                to: stealth_pool_info,
                space: StealthPool::LEN as u64,
                lamports,
                owner: &crate::ID,
            }
            .invoke_signed(&[signer])?;
        }
    } else {
        require!(
            stealth_pool_info.data_len() == StealthPool::LEN,
            ProgramError::InvalidAccountData
        );
    }

    require_eq_keys!(owner_program.address(), &crate::ID, ProgramError::IncorrectProgramId);
    require_eq_keys!(
        system_program.address(),
        &pinocchio_system::ID,
        ProgramError::IncorrectProgramId
    );

    let config = DelegateConfig {
        validator: args.validator().copied(),
        ..DelegateConfig::default()
    };
    if handle.len() <= StealthPool::MAX_HANDLE_SEED_BYTES {
        let seeds = [StealthPool::SEED, handle];

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
    } else {
        let seeds = [
            StealthPool::SEED,
            &handle[..StealthPool::MAX_HANDLE_SEED_BYTES],
            &handle[StealthPool::MAX_HANDLE_SEED_BYTES..],
        ];

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
}
