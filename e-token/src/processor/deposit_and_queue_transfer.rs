use core::{convert::TryFrom, marker::PhantomData};

use crate::processor::deposit_spl_tokens::transfer_to_vault_for_mint;
use crate::processor::utils::{read_mint_decimals, validate_token_account};
use crate::{assert_associated_token_address, assert_owner, assert_signer};
use ephemeral_spl_api::state::transfer_queue::{
    queue_len_for_mint_with_capacity, queue_push_from_data, QueuedTransfer,
    QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA, QUEUE_SEED,
};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

const MILLIS_PER_SECOND: u64 = 1_000;

#[inline(always)]
pub fn process_deposit_and_queue_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint, validator]
    // 1. []         Global vault PDA derived from [mint]
    // 2. []         Mint account
    // 3. [writable] User source token account
    // 4. [writable] Vault destination token account
    // 5. []         Destination token account (default) or destination owner (flagged)
    // 6. [signer]   Sender authority
    // 7. []         Token program
    let args = DepositAndQueueTransferArgs::try_from_bytes(instruction_data)?;

    let [queue_info, vault_info, mint_info, user_source_token_acc, vault_token_acc, destination_token_acc, user_authority, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(user_authority);
    assert_owner!(queue_info, &ephemeral_spl_api::program::id_address());

    let amount = args.amount();
    validate_deposit_and_queue_transfer_params(
        amount,
        args.min_delay_ms(),
        args.max_delay_ms(),
        args.split(),
    )?;

    let split = args.split() as usize;
    let (queue_len_before, validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        match queue_len_for_mint_with_capacity(data, mint_info.address(), split) {
            Ok(result) => result,
            Err(ProgramError::AccountDataTooSmall) => {
                #[cfg(feature = "logging")]
                pinocchio_log::log!("Queue is full, skipping transfer");
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    };

    let program_id = ephemeral_spl_api::program::id_address();
    let (derived_queue, _) = ephemeral_spl_api::Address::find_program_address(
        &[QUEUE_SEED, mint_info.address().as_ref(), validator.as_ref()],
        &program_id,
    );
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    #[cfg(not(feature = "logging"))]
    let _ = queue_len_before;

    let decimals = read_mint_decimals(mint_info, token_program_info)?;
    let inserted_at = queue_timestamp_now()?;

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
    let destination_owner = if args.should_create_destination_ata_idempotent() {
        *destination_token_acc.address()
    } else {
        let token = validate_token_account(
            destination_token_acc,
            mint_info.address(),
            None,
            Some(token_program_info.address()),
        )?;
        assert_associated_token_address!(
            destination_token_acc.address(),
            mint_info.address(),
            token.owner(),
            token_program_info.address()
        );
        *token.owner()
    };
    let split_plan = build_split_plan(amount, split, decimals)?;

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    for index in 0..split {
        let queued_amount = split_plan.amount_for_index(index);
        let queue_position = queue_len_before
            .checked_add(index)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let selected_delay_ms = choose_split_delay_ms(
            args.min_delay_ms(),
            args.max_delay_ms(),
            queue_position,
            &destination_owner,
        )?;
        let stored_delay = queue_delay_units_from_millis(selected_delay_ms)?;
        let ready_at = inserted_at
            .checked_add(stored_delay)
            .ok_or(ProgramError::InvalidInstructionData)?;

        queue_push_from_data(
            data,
            QueuedTransfer {
                source,
                destination_owner,
                amount: queued_amount,
                ready_at,
                inserted_at,
                task_id: 0,
                flags: args.flags(),
                _pad0: [0; 7],
            },
        )?;

        #[cfg(feature = "logging")]
        pinocchio_log::log!(
            "DepositAndQueueTransfer split {}/{} amount: {} delay_ms: {} ready_at: {}",
            index + 1,
            split,
            queued_amount,
            selected_delay_ms,
            ready_at
        );
    }

    #[cfg(feature = "logging")]
    pinocchio_log::log!(
        "DepositAndQueueTransfer queue length: {} -> {} delay_range_ms: {}..={}",
        queue_len_before,
        queue_len_before + split,
        args.min_delay_ms(),
        args.max_delay_ms()
    );

    Ok(())
}

pub struct DepositAndQueueTransferArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl DepositAndQueueTransferArgs<'_> {
    const LEN: usize = 28;
    const LEN_WITH_FLAGS: usize = 29;

    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositAndQueueTransferArgs<'_>, ProgramError> {
        if bytes.len() != Self::LEN && bytes.len() != Self::LEN_WITH_FLAGS {
            return Err(ProgramError::InvalidInstructionData);
        }

        if bytes.len() == Self::LEN_WITH_FLAGS
            && (bytes[Self::LEN] & !QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA) != 0
        {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(DepositAndQueueTransferArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        self.read_u64(0)
    }

    #[inline]
    pub fn min_delay_ms(&self) -> u64 {
        self.read_u64(8)
    }

    #[inline]
    pub fn max_delay_ms(&self) -> u64 {
        self.read_u64(16)
    }

    #[inline]
    pub fn split(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(24), buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }

    #[inline]
    pub fn flags(&self) -> u8 {
        if self.len == Self::LEN {
            0
        } else {
            unsafe { *self.raw.add(Self::LEN) }
        }
    }

    #[inline]
    pub fn should_create_destination_ata_idempotent(&self) -> bool {
        self.flags() & QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA != 0
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

#[inline(always)]
pub(crate) fn validate_deposit_and_queue_transfer_params(
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
    split: u32,
) -> ProgramResult {
    if amount == 0 || split == 0 || (split as u64) > amount {
        return Err(ProgramError::InvalidInstructionData);
    }
    if max_delay_ms < min_delay_ms {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(())
}

#[inline(always)]
fn queue_timestamp_now() -> Result<i64, ProgramError> {
    Clock::get()?
        .unix_timestamp
        .checked_mul(MILLIS_PER_SECOND as i64)
        .ok_or(ProgramError::InvalidInstructionData)
}

#[inline(always)]
fn queue_delay_units_from_millis(delay_ms: u64) -> Result<i64, ProgramError> {
    i64::try_from(delay_ms).map_err(|_| ProgramError::InvalidInstructionData)
}

struct SplitPlan {
    chunk_amount: u64,
    final_amount: u64,
    split: usize,
}

impl SplitPlan {
    #[inline(always)]
    fn amount_for_index(&self, index: usize) -> u64 {
        if index + 1 == self.split {
            self.final_amount
        } else {
            self.chunk_amount
        }
    }
}

#[inline(always)]
fn build_split_plan(amount: u64, split: usize, decimals: u8) -> Result<SplitPlan, ProgramError> {
    let default_chunk_amount = amount / split as u64;
    let default_final_amount = amount - (default_chunk_amount * (split as u64 - 1));

    let Some(preferred_quantum) = preferred_multiple_of_five_quantum(decimals) else {
        return Ok(SplitPlan {
            chunk_amount: default_chunk_amount,
            final_amount: default_final_amount,
            split,
        });
    };

    if let Some(chunk_amount) = preferred_equal_chunk(amount, split, preferred_quantum)? {
        return Ok(SplitPlan {
            chunk_amount,
            final_amount: chunk_amount,
            split,
        });
    }

    if split > 1 {
        if let Some(chunk_amount) = preferred_prefix_chunk(amount, split, preferred_quantum)? {
            let final_amount = amount
                .checked_sub(chunk_amount * (split as u64 - 1))
                .ok_or(ProgramError::InvalidInstructionData)?;
            return Ok(SplitPlan {
                chunk_amount,
                final_amount,
                split,
            });
        }
    }

    Ok(SplitPlan {
        chunk_amount: default_chunk_amount,
        final_amount: default_final_amount,
        split,
    })
}

#[inline(always)]
fn preferred_multiple_of_five_quantum(decimals: u8) -> Option<u64> {
    10_u64
        .checked_pow(u32::from(decimals))
        .and_then(|base_unit| base_unit.checked_mul(5))
}

#[inline(always)]
fn preferred_equal_chunk(
    amount: u64,
    split: usize,
    preferred_quantum: u64,
) -> Result<Option<u64>, ProgramError> {
    let split = u64::try_from(split).map_err(|_| ProgramError::InvalidInstructionData)?;
    let chunk_amount = largest_multiple_not_exceeding(amount / split, preferred_quantum);
    if chunk_amount == 0 {
        return Ok(None);
    }

    if chunk_amount
        .checked_mul(split)
        .ok_or(ProgramError::InvalidInstructionData)?
        == amount
    {
        Ok(Some(chunk_amount))
    } else {
        Ok(None)
    }
}

#[inline(always)]
fn preferred_prefix_chunk(
    amount: u64,
    split: usize,
    preferred_quantum: u64,
) -> Result<Option<u64>, ProgramError> {
    let prefix_count =
        u64::try_from(split - 1).map_err(|_| ProgramError::InvalidInstructionData)?;
    let chunk_amount = largest_multiple_not_exceeding(amount / prefix_count, preferred_quantum);
    if chunk_amount == 0 {
        return Ok(None);
    }

    let consumed = chunk_amount
        .checked_mul(prefix_count)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if consumed < amount {
        Ok(Some(chunk_amount))
    } else {
        Ok(None)
    }
}

#[inline(always)]
fn largest_multiple_not_exceeding(value: u64, quantum: u64) -> u64 {
    if quantum == 0 {
        0
    } else {
        (value / quantum) * quantum
    }
}

#[inline(always)]
fn choose_split_delay_ms(
    min_delay_ms: u64,
    max_delay_ms: u64,
    queue_position: usize,
    destination: &ephemeral_spl_api::Address,
) -> Result<u64, ProgramError> {
    if min_delay_ms == max_delay_ms {
        return Ok(min_delay_ms);
    }

    let delay_span = max_delay_ms
        .checked_sub(min_delay_ms)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let sample_space = delay_span
        .checked_add(1)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let queue_position =
        u64::try_from(queue_position).map_err(|_| ProgramError::InvalidInstructionData)?;

    min_delay_ms
        .checked_add(hash_delay_seed(destination, queue_position) % sample_space)
        .ok_or(ProgramError::InvalidInstructionData)
}

#[inline(always)]
fn hash_delay_seed(destination: &ephemeral_spl_api::Address, queue_position: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in destination.as_ref().iter() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash ^= queue_position;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    hash ^ (hash >> 32)
}
