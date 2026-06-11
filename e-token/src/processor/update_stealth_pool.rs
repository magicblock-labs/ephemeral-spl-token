use ephemeral_spl_api::instructions::UpdateStealthPoolArgs;
use ephemeral_spl_api::require_n_accounts;
use ephemeral_spl_api::state::stealth_pool::{StealthPool, StealthPoolFlags};
use ephemeral_spl_api::state::{Initializable, RawType};
use ephemeral_spl_api::{require, require_eq_keys};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use wheels::layout::Decodable as _;

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: []                  - Keypair : Payer.
///  1: [writable]          - PDA     : Stealth pool account.
///  2: [signer]            - Keypair : Pool authority.
///  3: []                  - Builtin : System program.
///
/// Instruction Data: UpdateStealthPoolArgs
///
#[inline(always)]
pub fn process_update_stealth_pool(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        _payer_info, // force multi-line
        stealth_pool_info,
        authority_info,
        _system_program_info,
    ] = require_n_accounts!(accounts, 4);

    let args = UpdateStealthPoolArgs::decode(instruction_data)?;

    require!(
        args.destinations().len() != 0
            && args.destinations().len() <= StealthPool::MAX_DESTINATIONS,
        ProgramError::InvalidInstructionData
    );
    require!(
        StealthPoolFlags::is_valid(args.flags()),
        ProgramError::InvalidInstructionData
    );
    require!(
        authority_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    let (derived_pool, bump) = StealthPool::find_pda(args.handle_hash());
    require_eq_keys!(
        &derived_pool,
        stealth_pool_info.address(),
        ProgramError::InvalidSeeds
    );

    require!(
        stealth_pool_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(
        stealth_pool_info.data_len() == StealthPool::LEN,
        ProgramError::InvalidAccountData
    );
    let data = unsafe { stealth_pool_info.borrow_unchecked() };
    let existing = bytemuck::try_from_bytes::<StealthPool>(data)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if existing.is_initialized() {
        require_eq_keys!(
            &existing.authority,
            authority_info.address(),
            ProgramError::IncorrectAuthority
        );
    }

    let data = unsafe { stealth_pool_info.borrow_unchecked_mut() };
    let pool = bytemuck::try_from_bytes_mut::<StealthPool>(data)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    *pool = StealthPool {
        discriminator: StealthPool::DISCRIMINATOR,
        bump,
        destination_count: args.destinations().len() as u8,
        flags: args.flags(),
        authority: *authority_info.address(),
        handle_hash: *args.handle_hash(),
        destinations: {
            let mut destinations = [Address::default(); StealthPool::MAX_DESTINATIONS];
            destinations[..args.destinations().len()].copy_from_slice(args.destinations());
            destinations
        },
    };

    Ok(())
}
