use core::{convert::TryFrom, marker::PhantomData};

use crate::processor::deposit_spl_tokens::transfer_to_vault_for_mint;
use ephemeral_spl_api::state::transfer_queue::{
    queue_push_from_data, queue_views_checked, QueuedTransfer, QUEUE_SEED,
};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

#[inline(always)]
pub fn process_deposit_and_queue_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint]
    // 1. []         Global vault PDA derived from [mint]
    // 2. []         Mint account
    // 3. [writable] User source token account
    // 4. [writable] Vault destination token account
    // 5. []         Destination token account recorded in the queue
    // 6. [signer]   Sender authority
    // 7. []         Token program
    let args = DepositAndQueueTransferArgs::try_from_bytes(instruction_data)?;

    let [queue_info, vault_info, mint_info, user_source_token_acc, vault_token_acc, destination_token_acc, user_authority, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !user_authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let amount = args.amount();
    let split = args.split() as usize;
    if amount == 0 || split == 0 || (split as u64) > amount {
        return Err(ProgramError::InvalidInstructionData);
    }

    let program_id = ephemeral_spl_api::program::id_address();
    let (derived_queue, _) = ephemeral_spl_api::Address::find_program_address(
        &[QUEUE_SEED, mint_info.address().as_ref()],
        &program_id,
    );
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    validate_destination_token_account(destination_token_acc, token_program_info, mint_info)?;
    let queue_len_before = validate_queue_capacity(queue_info, mint_info, split)?;
    #[cfg(not(feature = "logging"))]
    let _ = queue_len_before;

    let inserted_at = Clock::get()?.unix_timestamp;
    let delay_seconds =
        i64::try_from(args.delay_seconds()).map_err(|_| ProgramError::InvalidInstructionData)?;
    let ready_at = inserted_at
        .checked_add(delay_seconds)
        .ok_or(ProgramError::InvalidInstructionData)?;

    transfer_to_vault_for_mint(
        vault_info,
        mint_info,
        user_source_token_acc,
        vault_token_acc,
        user_authority,
        token_program_info,
        mint_info.address(),
        amount,
    )?;

    let source = *user_authority.address();
    let destination = *destination_token_acc.address();
    let split_amount = amount / split as u64;
    let last_amount = amount - (split_amount * (split as u64 - 1));

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    for index in 0..split {
        let queued_amount = if index + 1 == split {
            last_amount
        } else {
            split_amount
        };

        queue_push_from_data(
            data,
            QueuedTransfer {
                source,
                destination,
                amount: queued_amount,
                ready_at,
                inserted_at,
                task_id: 0,
            },
        )?;
    }

    #[cfg(feature = "logging")]
    pinocchio_log::log!(
        "DepositAndQueueTransfer queue length: {} -> {}",
        queue_len_before,
        queue_len_before + split
    );

    Ok(())
}

#[inline(always)]
fn validate_destination_token_account(
    destination_token_acc: &AccountView,
    token_program_info: &AccountView,
    mint_info: &AccountView,
) -> ProgramResult {
    if !destination_token_acc.owned_by(token_program_info.address()) {
        return Err(ProgramError::IllegalOwner);
    }

    let destination_token_data = unsafe { destination_token_acc.borrow_unchecked() };
    if destination_token_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let destination_token = unsafe { TokenAccount::from_bytes_unchecked(destination_token_data) };
    if destination_token.mint() != mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

#[inline(always)]
fn validate_queue_capacity(
    queue_info: &AccountView,
    mint_info: &AccountView,
    required_slots: usize,
) -> Result<usize, ProgramError> {
    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, items) = queue_views_checked(data)?;
    if header.mint != *mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let queue_len = header.length as usize;
    let available_slots = items.len() - queue_len;
    if available_slots < required_slots {
        return Err(ProgramError::AccountDataTooSmall);
    }

    Ok(queue_len)
}

pub struct DepositAndQueueTransferArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl DepositAndQueueTransferArgs<'_> {
    const LEN: usize = 20;

    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositAndQueueTransferArgs<'_>, ProgramError> {
        if bytes.len() != Self::LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(DepositAndQueueTransferArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        self.read_u64(0)
    }

    #[inline]
    pub fn delay_seconds(&self) -> u64 {
        self.read_u64(8)
    }

    #[inline]
    pub fn split(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(16), buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }

    #[inline]
    fn read_u64(&self, offset: usize) -> u64 {
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(offset), buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }
}
