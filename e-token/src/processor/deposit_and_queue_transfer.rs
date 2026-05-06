use crate::processor::execute_transfer_callback::derive_group_receipt_id;
use crate::processor::internal::token_vault::transfer_to_vault_for_mint;
use crate::processor::utils::{group_receipt_create, read_mint_decimals, GroupReceiptAccounts};
use alloc::vec;
use alloc::vec::Vec;
use core::convert::TryFrom;
use data_layout::variable_offset_layout;
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::debug_log;
#[cfg(feature = "logging")]
use ephemeral_spl_api::state::transfer_queue::capacity_from_data_len;
use ephemeral_spl_api::state::transfer_queue::{
    queue_len_and_bump_for_mint_with_capacity, queue_push_from_data, QueuedTransfer, TransferQueue,
    QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::address::address_eq;
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::instructions::TransferChecked;

const MILLIS_PER_SECOND: u64 = 1_000;

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: [writable]          - PDA     : Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator]).
///  1: []                  - PDA     : Global vault account (PDA derived from [mint]).
///  2: []                  - SPL     : Mint account.
///  3: [writable]          - SPL     : User source token account.
///  4: [writable]          - SPL     : Vault destination token account.
///  5: []                  - Any     : Destination owner.
///  6: [signer]            - Keypair : Sender authority.
///  7: []                  - SPL     : Token program.
///  8: [writable]          - SPL     : Reimbursement token account.
///  9: [writable]          - SPL     : Group receipt.
///  10: [writable]         - SPL     : Magic vault
///  11: []                 - Magic   : Magic program
///
/// Instruction Data: DepositAndQueueTransferArgs
///
#[inline(always)]
pub fn process_deposit_and_queue_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        queue_info, // force multi-line
        vault_info,
        mint_info,
        user_source_token_acc,
        vault_token_acc,
        destination_info,
        user_authority,
        token_program_info,
        reimbursement_token_info,
        group_receipt_info,
        magic_vault,
        magic_program,
    ] = require_n_accounts!(accounts, 12);

    pinocchio_log::log!("instruction_data: {}", instruction_data.len());
    let args = DepositAndQueueTransferArgs::decode(instruction_data)?;

    let group_id = args.group_id_u32();
    let (group_receipt, group_receipt_bump) =
        derive_group_receipt_id(queue_info.address(), user_authority.address(), group_id);

    require!(
        group_receipt.eq(group_receipt_info.address()),
        ProgramError::InvalidInstructionData
    );
    require!(
        user_authority.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        queue_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(
        magic_program.address().eq(&MAGIC_PROGRAM_ID),
        ProgramError::IncorrectProgramId
    );

    let amount = args.amount();
    validate_deposit_and_queue_transfer_params(
        amount,
        args.min_delay_ms(),
        args.max_delay_ms(),
        args.split(),
    )?;

    let split = args.split() as usize;
    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let (queue_len_before, validator, bump) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        match queue_len_and_bump_for_mint_with_capacity(data, mint_info.address(), split) {
            Ok((queue_len_before, validator, bump)) => (queue_len_before, validator, bump),
            Err(ProgramError::AccountDataTooSmall) => {
                debug_log!("Queue is full");
                if !address_eq(reimbursement_token_info.address(), &crate::ID) {
                    TransferChecked {
                        mint: mint_info,
                        from: user_source_token_acc,
                        to: reimbursement_token_info,
                        authority: user_authority,
                        token_program: token_program_info.address(),
                        amount,
                        decimals,
                    }
                    .invoke()?;
                }
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    };

    let derived_queue = TransferQueue::derive_pda(mint_info.address(), &validator, bump)?;
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

    let now_ms = queue_timestamp_now()?;

    require!(
        !address_eq(
            unsafe { destination_info.owner() },
            token_program_info.address()
        ),
        ProgramError::InvalidAccountData
    );

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
    let destination_owner = *destination_info.address();
    let client_ref_id = args.client_ref_id().unwrap_or(0);
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
        let ready_at = now_ms
            .checked_add(stored_delay)
            .ok_or(ProgramError::InvalidInstructionData)?;
        let mut queued_transfer = QueuedTransfer {
            source,
            destination_owner,
            amount: queued_amount,
            ready_at,
            client_ref_id,
            task_id: 0,
            flags: args.flags().unwrap_or(0) | QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
            group_id: [0; 3],
        };
        queued_transfer.set_group_id(group_id)?;

        queue_push_from_data(data, queued_transfer)?;

        debug_log!(
            "DepositAndQueueTransfer split {}/{} group_id: {} task_id: {} client_ref_id: {} amount: {} delay_ms: {} ready_at: {}",
            index + 1,
            split,
            group_id,
            ephemeral_spl_api::state::transfer_queue::queue_peek_next_task_id_from_data(data)?,
            client_ref_id,
            queued_amount,
            selected_delay_ms,
            ready_at
        );
    }

    debug_log!(
        "DepositAndQueueTransfer queue length: {} -> {} capacity: {} delay_range_ms: {}..={}",
        queue_len_before,
        queue_len_before + split,
        capacity_from_data_len(data.len()),
        args.min_delay_ms(),
        args.max_delay_ms()
    );

    group_receipt_create(
        &GroupReceiptAccounts {
            queue_info,
            group_receipt_info,
            source: user_authority,
            magic_vault,
            _magic_program: magic_program,
        },
        group_receipt_bump,
        group_id,
        args.split(),
    )?;

    Ok(())
}

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DepositAndQueueTransferArgs {
    pub amount: u64,
    pub group_id: [u8; 3],
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub split: u32,
    pub flags: Option<u8>,
    pub client_ref_id: Option<u64>,
}

static_assertions::const_assert!(matches!(
    DepositAndQueueTransferArgs::DATA_LENS,
    [31, 32, 39, 40]
));

impl DepositAndQueueTransferArgsView<'_> {
    pub fn group_id_u32(&self) -> u32 {
        let id = self.group_id();
        u32::from(id[0]) | (u32::from(id[1]) << 8) | (u32::from(id[2]) << 16)
    }
}

#[inline(always)]
fn validate_deposit_and_queue_transfer_params(
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
    split: u32,
) -> ProgramResult {
    require!(
        amount != 0 && split != 0 && (split as u64) <= amount,
        ProgramError::InvalidInstructionData
    );
    require!(
        max_delay_ms >= min_delay_ms,
        ProgramError::InvalidInstructionData
    );

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
