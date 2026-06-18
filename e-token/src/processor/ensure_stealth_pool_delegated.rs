use ephemeral_rollups_pinocchio::{
    acl::{
        consts::PERMISSION_PROGRAM_ID, instruction::CreatePermissionCpiBuilder,
        pda::permission_pda_from_permissioned_account, types::MembersArgs,
    },
    instruction::DelegateAccountCpiBuilder,
    types::DelegateConfig,
};
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
///  2: [writable]          - PDA     : Stealth pool permission account.
///  3: []                  - Program : Owner program (this program).
///  4: [writable]          - PDA     : Buffer account.
///  5: [writable]          - PDA     : Delegation record account.
///  6: [writable]          - PDA     : Delegation metadata account.
///  7: []                  - Program : Delegation program.
///  8: []                  - Builtin : System program.
///  9: []                  - Program : Permission program (ACL).
///
/// Instruction Data: EnsureStealthPoolDelegatedArgs
///
pub fn process_ensure_stealth_pool_delegated(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let [
        payer_info, // force multi-line
        stealth_pool_info,
        stealth_pool_permission_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        delegation_program_info,
        system_program,
        permission_program,
    ] = require_n_accounts!(accounts, 10);

    let args = EnsureStealthPoolDelegatedArgs::decode(instruction_data)?;

    require!(payer_info.is_signer(), ProgramError::MissingRequiredSignature);

    let handle = StealthPool::handle_from_storage(args.handle())?;
    let (derived_pool, bump) = StealthPool::find_pda(handle)?;
    require_eq_keys!(&derived_pool, stealth_pool_info.address(), ProgramError::InvalidSeeds);

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    require_eq_keys!(owner_program.address(), &crate::ID, ProgramError::IncorrectProgramId);
    require_eq_keys!(
        delegation_program_info.address(),
        &delegation_program,
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(
        system_program.address(),
        &pinocchio_system::ID,
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(
        permission_program.address(),
        &PERMISSION_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );

    let stealth_pool_delegated = stealth_pool_info.owned_by(&delegation_program);
    if !stealth_pool_delegated && !stealth_pool_info.owned_by(&crate::ID) {
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
    } else if !stealth_pool_delegated {
        require!(
            stealth_pool_info.data_len() == StealthPool::LEN,
            ProgramError::InvalidAccountData
        );
    }

    let expected_permission = permission_pda_from_permissioned_account(stealth_pool_info.address());
    require_eq_keys!(
        &expected_permission,
        stealth_pool_permission_info.address(),
        ProgramError::InvalidSeeds
    );

    let stealth_pool_permission_exists = stealth_pool_permission_info.lamports() > 0;
    require!(
        !stealth_pool_permission_exists || stealth_pool_permission_info.owned_by(&PERMISSION_PROGRAM_ID),
        ProgramError::IncorrectProgramId
    );

    if !stealth_pool_permission_exists {
        if handle.len() <= StealthPool::MAX_HANDLE_SEED_BYTES {
            let seeds = [StealthPool::SEED, handle];
            CreatePermissionCpiBuilder::new(
                stealth_pool_info,
                stealth_pool_permission_info,
                payer_info,
                system_program,
                &PERMISSION_PROGRAM_ID,
            )
            .members(MembersArgs { members: Some(&[]) })
            .seeds(&seeds)
            .bump(bump)
            .invoke()?;
        } else {
            let seeds = [
                StealthPool::SEED,
                &handle[..StealthPool::MAX_HANDLE_SEED_BYTES],
                &handle[StealthPool::MAX_HANDLE_SEED_BYTES..],
            ];
            CreatePermissionCpiBuilder::new(
                stealth_pool_info,
                stealth_pool_permission_info,
                payer_info,
                system_program,
                &PERMISSION_PROGRAM_ID,
            )
            .members(MembersArgs { members: Some(&[]) })
            .seeds(&seeds)
            .bump(bump)
            .invoke()?;
        }
    }

    if stealth_pool_delegated {
        return Ok(());
    }

    require!(
        stealth_pool_info.data_len() == StealthPool::LEN,
        ProgramError::InvalidAccountData
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
