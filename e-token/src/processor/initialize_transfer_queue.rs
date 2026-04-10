use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID, instruction::CreatePermissionCpiBuilder,
    pda::permission_pda_from_permissioned_account, types::MembersArgs,
};
use ephemeral_spl_api::consts::TRANSFER_QUEUE_INITIAL_BUFFER_LAMPORTS;
use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, header_len, init_queue, item_len, queue_views_mut_checked,
    TransferQueue,
};
use pinocchio::address::address_eq;
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::{CreateAccount, Transfer};

pub const DEFAULT_TRANSFER_QUEUE_ITEMS: u32 = 100;
/// Default queue size in bytes. (HEADER_LEN + ITEM_LEN * DEFAULT_TRANSFER_QUEUE_ITEMS)
pub const DEFAULT_TRANSFER_QUEUE_SIZE_BYTES: u64 =
    (header_len() + item_len() * DEFAULT_TRANSFER_QUEUE_ITEMS as usize) as u64;

#[inline(always)]
pub fn process_initialize_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer (funds account creation)
    // 1. [writable] Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator])
    // 2. [writable] Transfer queue permission account
    // 3. []         Mint account (seed)
    // 4. []         Validator
    // 5. []         System program
    // 6. []         Permission program (ACL)
    let args = InitializeTransferQueueArgs::try_from_bytes(instruction_data)?;

    let [payer_info, queue_info, queue_permission_info, mint_info, validator_info, system_program_info, permission_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let (requested_items, queue_size) = if let Some(items) = args.requested_items() {
        (items, header_len() + item_len() * items as usize)
    } else {
        (
            DEFAULT_TRANSFER_QUEUE_ITEMS,
            DEFAULT_TRANSFER_QUEUE_SIZE_BYTES as usize,
        )
    };
    if requested_items == 0 {
        return Err(ProgramError::InvalidInstructionData);
    };

    let program_id = crate::ID;
    let (derived_queue, bump) =
        TransferQueue::find_pda(mint_info.address(), validator_info.address());
    if !address_eq(&derived_queue, queue_info.address()) {
        return Err(ProgramError::InvalidSeeds);
    }

    if !address_eq(permission_program_info.address(), &PERMISSION_PROGRAM_ID) {
        return Err(ProgramError::IncorrectProgramId);
    }
    let expected_permission = permission_pda_from_permissioned_account(queue_info.address());
    if !address_eq(&expected_permission, queue_permission_info.address()) {
        return Err(ProgramError::InvalidSeeds);
    }
    let queue_permission_exists = queue_permission_info.lamports() > 0;
    if queue_permission_exists && !queue_permission_info.owned_by(&PERMISSION_PROGRAM_ID) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let rent = Rent::get()?;
    let target_lamports = rent
        .try_minimum_balance(queue_size)?
        .checked_add(TRANSFER_QUEUE_INITIAL_BUFFER_LAMPORTS)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if !queue_info.owned_by(&program_id) {
        if queue_info.lamports() > 0 {
            return Err(ProgramError::IllegalOwner);
        }
        if !payer_info.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

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
        if !payer_info.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        CreatePermissionCpiBuilder::new(
            queue_info,
            queue_permission_info,
            payer_info,
            system_program_info,
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
    if capacity_from_data_len(data_len) == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    init_queue(data, bump, *mint_info.address(), *validator_info.address())?;

    let (header, _) = queue_views_mut_checked(data)?;
    if header.bump != bump
        || !address_eq(&header.mint, mint_info.address())
        || !address_eq(&header.validator, validator_info.address())
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

pub struct InitializeTransferQueueArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeTransferQueueArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeTransferQueueArgs<'_>, ProgramError> {
        if !bytes.is_empty() && bytes.len() != 4 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeTransferQueueArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn requested_items(&self) -> Option<u32> {
        if self.len == 0 {
            None
        } else {
            let mut buf = [0u8; 4];
            unsafe {
                core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
            }
            Some(u32::from_le_bytes(buf))
        }
    }
}
