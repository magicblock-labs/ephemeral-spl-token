#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::{
    intent_bundle::{ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta},
    spl::consts::TOKEN_PROGRAM_ID,
};
use ephemeral_spl_api::instruction::internal::{
    EXECUTE_READY_QUEUED_TRANSFER, MARK_TRANSFER_QUEUE_REFILL_PENDING,
};
use ephemeral_spl_api::state::transfer_queue::{
    queue_peek_from_data, queue_pop_from_data, queue_views_checked, QueuedTransfer, QUEUE_SEED,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::{clock::Clock, Sysvar};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;

use crate::{
    assert_owner,
    processor::{
        rent_pda::RENT_PDA,
        transfer_queue_refill::{
            queue_refill_state_address, refill_transfer_queue_amounts,
            MARK_TRANSFER_QUEUE_REFILL_PENDING_COMPUTE_UNITS,
            MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX,
        },
    },
};

pub(crate) const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;

const ASSOCIATED_TOKEN_PROGRAM_ID: ephemeral_spl_api::Address =
    pinocchio_associated_token_account::ID;
const EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS: u32 = 140_000;
const MAGIC_INTENT_BUNDLE_DATA_LEN: usize = 512;
const MILLIS_PER_SECOND: i64 = 1_000;

struct TickAccounts<'a> {
    queue_info: &'a AccountView,
    magic_fee_vault_info: &'a AccountView,
    magic_context_info: &'a AccountView,
    magic_program_info: &'a AccountView,
}

struct QueueTickState {
    mint: ephemeral_spl_api::Address,
    queue_bump: u8,
    queue_len: usize,
    validator: ephemeral_spl_api::Address,
    queued_transfer: Option<QueuedTransfer>,
}

#[inline(always)]
pub fn process_transfer_queue_tick(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    let tick_accounts = parse_tick_accounts(accounts)?;
    let program_id = crate::ID;
    let clock = Clock::get()?;
    let queue_state = read_queue_tick_state(tick_accounts.queue_info, &program_id)?;

    if try_schedule_queue_refill(&tick_accounts, &queue_state)? {
        return Ok(());
    }

    let Some(queued_transfer) =
        ready_queued_transfer(queue_state.queued_transfer, queue_state.queue_len, &clock)?
    else {
        return Ok(());
    };

    schedule_execute_ready_transfer(&tick_accounts, &queue_state, &queued_transfer, &program_id)?;
    pop_executed_transfer(tick_accounts.queue_info, queued_transfer)
}

#[inline(always)]
fn derive_associated_token_address(
    wallet: &ephemeral_spl_api::Address,
    mint: &ephemeral_spl_api::Address,
) -> ephemeral_spl_api::Address {
    ephemeral_spl_api::Address::find_program_address(
        &[wallet.as_ref(), TOKEN_PROGRAM_ID.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

#[inline(always)]
fn parse_tick_accounts(accounts: &[AccountView]) -> Result<TickAccounts<'_>, ProgramError> {
    // Expected accounts:
    // 0. [writable] Transfer queue PDA, used as the scheduled-action authority
    // 1. [writable] Validator magic fee vault PDA derived from ["magic-fee-vault", validator]
    // 2. [writable] Magic context account
    // 3. []         Magic program
    let [queue_info, magic_fee_vault_info, magic_context_info, magic_program_info, ..] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if magic_program_info.address() != &ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    Ok(TickAccounts {
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    })
}

#[inline(always)]
fn read_queue_tick_state(
    queue_info: &AccountView,
    program_id: &ephemeral_spl_api::Address,
) -> Result<QueueTickState, ProgramError> {
    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    let mint = header.mint;
    let validator = header.validator;
    let queue_len = header.length as usize;

    let (derived_queue, queue_bump) = ephemeral_spl_api::Address::find_program_address(
        &[QUEUE_SEED, mint.as_ref(), validator.as_ref()],
        program_id,
    );
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(QueueTickState {
        mint,
        queue_bump,
        queue_len,
        validator,
        queued_transfer: queue_peek_from_data(data)?,
    })
}

#[inline(always)]
fn try_schedule_queue_refill(
    tick_accounts: &TickAccounts<'_>,
    queue_state: &QueueTickState,
) -> Result<bool, ProgramError> {
    let (queue_rent_exemption, refill_lamports) =
        refill_transfer_queue_amounts(tick_accounts.queue_info.data_len())?;
    let refill_threshold = queue_rent_exemption
        .checked_add(refill_lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    if tick_accounts.queue_info.lamports() >= refill_threshold {
        return Ok(false);
    }

    let refill_data = [
        MARK_TRANSFER_QUEUE_REFILL_PENDING,
        MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX,
    ];
    let refill_accounts = [
        ShortAccountMeta {
            pubkey: RENT_PDA,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: queue_refill_state_address(tick_accounts.queue_info.address()),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: SYSTEM_PROGRAM_ID,
            is_writable: false,
        },
    ];
    let standalone_actions = [CallHandler {
        destination_program: crate::ID,
        escrow_authority: tick_accounts.queue_info.clone(),
        args: ActionArgs::new(&refill_data)
            .with_escrow_index(MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX),
        compute_units: MARK_TRANSFER_QUEUE_REFILL_PENDING_COMPUTE_UNITS,
        accounts: &refill_accounts,
    }];

    invoke_queue_standalone_action(tick_accounts, queue_state, &standalone_actions)?;
    Ok(true)
}

#[inline(always)]
fn ready_queued_transfer(
    queued_transfer: Option<QueuedTransfer>,
    queue_len: usize,
    clock: &Clock,
) -> Result<Option<QueuedTransfer>, ProgramError> {
    let Some(queued_transfer) = queued_transfer else {
        #[cfg(feature = "logging")]
        pinocchio_log::log!("ProcessTransferQueueTick queue length: {}", queue_len);
        return Ok(None);
    };

    #[cfg(not(feature = "logging"))]
    let _ = queue_len;

    let now = clock
        .unix_timestamp
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if queued_transfer.ready_at > now {
        #[cfg(feature = "logging")]
        pinocchio_log::log!(
            "ProcessTransferQueueTick queue length: {} (next not ready)",
            queue_len
        );
        return Ok(None);
    }

    Ok(Some(queued_transfer))
}

#[inline(always)]
fn schedule_execute_ready_transfer(
    tick_accounts: &TickAccounts<'_>,
    queue_state: &QueueTickState,
    queued_transfer: &QueuedTransfer,
    program_id: &ephemeral_spl_api::Address,
) -> ProgramResult {
    assert_owner!(tick_accounts.queue_info, program_id);

    #[cfg(feature = "logging")]
    pinocchio_log::log!(
        "ProcessTransferQueueTick queue length before pop: {}",
        queue_state.queue_len
    );

    let (vault, _) =
        ephemeral_spl_api::Address::find_program_address(&[queue_state.mint.as_ref()], program_id);
    let vault_token_account = derive_associated_token_address(&vault, &queue_state.mint);
    let destination_token_account =
        derive_associated_token_address(&queued_transfer.destination_owner, &queue_state.mint);

    let mut execute_data = [0_u8; 19];
    execute_data[0] = EXECUTE_READY_QUEUED_TRANSFER;
    execute_data[1] = EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX;
    execute_data[2..10].copy_from_slice(&queued_transfer.amount.to_le_bytes());
    execute_data[10] = queued_transfer.flags;
    let execute_data_len = if queued_transfer.client_ref_id != 0 {
        execute_data[11..19].copy_from_slice(&queued_transfer.client_ref_id.to_le_bytes());
        19
    } else {
        11
    };

    let execute_accounts = [
        ShortAccountMeta {
            pubkey: vault,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: queue_state.mint,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: vault_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: queued_transfer.destination_owner,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: destination_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: RENT_PDA,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: TOKEN_PROGRAM_ID,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: ASSOCIATED_TOKEN_PROGRAM_ID,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: SYSTEM_PROGRAM_ID,
            is_writable: false,
        },
    ];
    let standalone_actions = [CallHandler {
        destination_program: crate::ID,
        escrow_authority: tick_accounts.queue_info.clone(),
        args: ActionArgs::new(&execute_data[..execute_data_len])
            .with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
        compute_units: EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
        accounts: &execute_accounts,
    }];

    invoke_queue_standalone_action(tick_accounts, queue_state, &standalone_actions)
}

#[inline(always)]
fn invoke_queue_standalone_action(
    tick_accounts: &TickAccounts<'_>,
    queue_state: &QueueTickState,
    standalone_actions: &[CallHandler],
) -> ProgramResult {
    let queue_bump_seed = [queue_state.queue_bump];
    let signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(queue_state.mint.as_ref()),
        Seed::from(queue_state.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let signers = [Signer::from(&signer_seeds)];
    let mut intent_bundle_data = [0_u8; MAGIC_INTENT_BUNDLE_DATA_LEN];
    let derived_magic_fee_vault =
        magic_fee_vault_pda_from_validator(&queue_state.validator.to_bytes().into());
    if derived_magic_fee_vault.to_bytes() != tick_accounts.magic_fee_vault_info.address().to_bytes()
    {
        return Err(ProgramError::InvalidSeeds);
    }

    MagicIntentBundleBuilder::new(
        tick_accounts.queue_info.clone(),
        tick_accounts.magic_context_info.clone(),
        tick_accounts.magic_program_info.clone(),
    )
    .magic_fee_vault(tick_accounts.magic_fee_vault_info.clone())
    .set_standalone_actions(standalone_actions)
    .build_and_invoke_signed(&mut intent_bundle_data, &signers)
}

#[inline(always)]
fn pop_executed_transfer(
    queue_info: &AccountView,
    queued_transfer: QueuedTransfer,
) -> ProgramResult {
    let data = unsafe { queue_info.borrow_unchecked_mut() };
    let popped_transfer = queue_pop_from_data(data)?.ok_or(ProgramError::InvalidAccountData)?;
    if popped_transfer.task_id != queued_transfer.task_id {
        return Err(ProgramError::InvalidAccountData);
    }

    #[cfg(feature = "logging")]
    {
        let sender = popped_transfer.source.to_string();
        let receiver = popped_transfer.destination_owner.to_string();
        pinocchio_log::log!(
            "ProcessTransferQueueTick group_id: {} task_id: {} client_ref_id: {} sender: {} receiver: {} amount: {}",
            popped_transfer.group_id(),
            popped_transfer.task_id,
            popped_transfer.client_ref_id,
            sender.as_str(),
            receiver.as_str(),
            popped_transfer.amount
        );
    }

    Ok(())
}
