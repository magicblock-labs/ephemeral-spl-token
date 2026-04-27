use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::CreatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account, types::MembersArgs,
};
use ephemeral_spl_api::consts::TRANSFER_QUEUE_INITIAL_BUFFER_LAMPORTS;
use ephemeral_spl_api::require_n_accounts;
use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, init_queue, queue_views_checked, TransferQueue, HEADER_LEN, ITEM_LEN,
};
use ephemeral_spl_api::{require, require_eq_keys};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::{CreateAccount, Transfer};

pub const DEFAULT_TRANSFER_QUEUE_ITEMS: u32 = 100;
/// Default queue size in bytes. (HEADER_LEN + ITEM_LEN * DEFAULT_TRANSFER_QUEUE_ITEMS)
pub const DEFAULT_TRANSFER_QUEUE_SIZE_BYTES: u64 =
    (HEADER_LEN + ITEM_LEN * DEFAULT_TRANSFER_QUEUE_ITEMS as usize) as u64;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator]).
///  2: [writable]          - PDA     : Transfer queue permission account.
///  3: []                  - SPL     : Mint account.
///  4: []                  - Any     : Validator.
///  5: []                  - Builtin : System program.
///  6: []                  - Program : Permission program (ACL).
///
/// Instruction Data: InitializeTransferQueueArgs
///
#[inline(always)]
pub fn process_initialize_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        queue_info,
        queue_permission_info,
        mint_info,
        validator_info,
        system_program_info,
        permission_program_info,
    ] = require_n_accounts!(accounts, 7);

    let args = InitializeTransferQueueArgs::decode(instruction_data)?;

    let (requested_items, queue_size) = if let Some(items) = args.requested_items() {
        (items, HEADER_LEN + ITEM_LEN * items as usize)
    } else {
        (
            DEFAULT_TRANSFER_QUEUE_ITEMS,
            DEFAULT_TRANSFER_QUEUE_SIZE_BYTES as usize,
        )
    };
    require!(requested_items != 0, ProgramError::InvalidInstructionData);

    let program_id = crate::ID;
    let (derived_queue, bump) =
        TransferQueue::find_pda(mint_info.address(), validator_info.address());
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

    // permission_program_info is needed by CreatePermissionCpiBuilder
    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission_program_info.address(),
        ProgramError::IncorrectProgramId
    );

    let expected_permission = permission_pda_from_permissioned_account(queue_info.address());
    require_eq_keys!(
        &expected_permission,
        queue_permission_info.address(),
        ProgramError::InvalidSeeds
    );

    let queue_permission_exists = queue_permission_info.lamports() > 0;
    require!(
        !queue_permission_exists || queue_permission_info.owned_by(&PERMISSION_PROGRAM_ID),
        ProgramError::IncorrectProgramId
    );

    let rent = Rent::get()?;
    let target_lamports = rent
        .try_minimum_balance(queue_size)?
        .checked_add(TRANSFER_QUEUE_INITIAL_BUFFER_LAMPORTS)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if !queue_info.owned_by(&program_id) {
        require!(queue_info.lamports() == 0, ProgramError::IllegalOwner);
        require!(
            payer_info.is_signer(),
            ProgramError::MissingRequiredSignature
        );

        let bump_seed = [bump];
        let signer_seeds =
            TransferQueue::signer_seeds(mint_info.address(), validator_info.address(), &bump_seed);
        let signer = Signer::from(&signer_seeds);

        CreateAccount {
            from: payer_info,
            to: queue_info,
            space: (queue_size as u64).min(DEFAULT_TRANSFER_QUEUE_SIZE_BYTES),
            lamports: target_lamports,
            owner: &program_id,
        }
        .invoke_signed(&[signer])?;
    }

    if !queue_permission_exists {
        require!(
            payer_info.is_signer(),
            ProgramError::MissingRequiredSignature
        );
        CreatePermissionCpiBuilder::new(
            queue_info,
            queue_permission_info,
            payer_info,
            system_program_info, // CreatePermission ix validates this
            &PERMISSION_PROGRAM_ID,
        )
        .members(MembersArgs { members: Some(&[]) })
        .seeds(&TransferQueue::seeds(
            mint_info.address(),
            validator_info.address(),
        ))
        .bump(bump)
        .invoke()?;
    }

    let shortfall = target_lamports.saturating_sub(queue_info.lamports());
    if shortfall > 0 {
        Transfer {
            from: payer_info,
            to: queue_info,
            lamports: shortfall,
        }
        .invoke()?;
    }

    let data_len = queue_info.data_len();
    require!(
        capacity_from_data_len(data_len) != 0,
        ProgramError::InvalidAccountData
    );

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    init_queue(data, bump, *mint_info.address(), *validator_info.address())?;

    let (header, _) = queue_views_checked(data)?;
    require!(
        header.bump == bump
            && header.mint == *mint_info.address()
            && header.validator == *validator_info.address(),
        ProgramError::InvalidAccountData
    );

    Ok(())
}

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct InitializeTransferQueueArgs {
    pub requested_items: Option<u32>,
}

static_assertions::const_assert!(matches!(InitializeTransferQueueArgs::DATA_LENS, [0, 4]));
