#[cfg(feature = "logging")]
use alloc::string::ToString;

use crate::processor::internal::group_receipt::{derive_group_receipt_id, TransferCallbackArgs};
use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_rollups_pinocchio::{
    intent_bundle::{
        ActionArgs, ActionCallback, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
    },
    spl::consts::TOKEN_PROGRAM_ID,
};
use ephemeral_spl_api::debug_log;
use ephemeral_spl_api::require_n_accounts;
use ephemeral_spl_api::state::transfer_queue::{
    queue_peek_from_data, queue_pop_from_data, queue_views_checked, QueuedTransfer, QUEUE_SEED,
};
use ephemeral_spl_api::{require, require_eq_keys};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::{clock::Clock, Sysvar};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;

use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::internal;
use crate::processor::internal::transfer_queue_refill::{
    queue_refill_state_address, refill_transfer_queue_amounts,
    MARK_TRANSFER_QUEUE_REFILL_PENDING_COMPUTE_UNITS,
    MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX,
};
use crate::processor::internal::ASSOCIATED_TOKEN_PROGRAM_ID;
use crate::processor::utils::{
    create_action_accounts, execute_queued_transfer_action, invoke_standalone_transfer_action,
    MagicAccounts, MagicState, CALLBACK_SIGNER, MAGIC_VAULT_ID,
};
use crate::{
    instruction::ESplInternalInstruction,
    processor::execute_ready_queued_transfer::ExecuteQueuedTransferArgs,
};

const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;

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

///
/// Executes on: BASE only.
///
/// Accounts:
///
///  0: [writable]          - PDA     : Transfer queue account, used as the scheduled-action authority.
///  1: [writable]          - PDA     : Validator magic fee vault PDA derived from ["magic-fee-vault", validator].
///  2: [writable]          - Any     : Magic context account.
///  3: []                  - Program : Magic program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_transfer_queue_tick(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    require!(
        instruction_data.is_empty(),
        ProgramError::InvalidInstructionData
    );

    let [
        queue_info, // force multi-line
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    ] = require_n_accounts!(accounts, 4);

    require_eq_keys!(
        magic_program_info.address(),
        &MAGIC_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );

    let tick_accounts = TickAccounts {
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    };
    let program_id = crate::ID;
    let clock = Clock::get()?;
    let queue_state = read_queue_tick_state(tick_accounts.queue_info, &program_id)?;

    // this instruction is currently permissionless (anyone can invoke it)
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
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

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

    let refill_data = ESplInternalInstruction::MarkTransferQueueRefillPending
        .with_data(&[MARK_TRANSFER_QUEUE_REFILL_PENDING_ESCROW_INDEX]);
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
        callback: None,
    }];

    invoke_standalone_transfer_action(
        &MagicAccounts {
            queue_info: tick_accounts.queue_info,
            magic_fee_vault_info: tick_accounts.magic_fee_vault_info,
            magic_context_info: tick_accounts.magic_context_info,
            magic_program_info: tick_accounts.magic_program_info,
        },
        &MagicState {
            mint: queue_state.mint,
            queue_bump: queue_state.queue_bump,
            validator: queue_state.validator,
        },
        &standalone_actions,
    )?;
    Ok(true)
}

#[inline(always)]
fn ready_queued_transfer(
    queued_transfer: Option<QueuedTransfer>,
    _queue_len: usize,
    clock: &Clock,
) -> Result<Option<QueuedTransfer>, ProgramError> {
    let Some(queued_transfer) = queued_transfer else {
        debug_log!("ProcessTransferQueueTick queue length: {}", _queue_len);
        return Ok(None);
    };

    let now = clock
        .unix_timestamp
        .checked_mul(MILLIS_PER_SECOND)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if queued_transfer.ready_at > now {
        debug_log!(
            "ProcessTransferQueueTick queue length: {} (next not ready)",
            _queue_len
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
    require!(
        tick_accounts.queue_info.owned_by(program_id),
        ProgramError::InvalidAccountOwner
    );

    debug_log!(
        "ProcessTransferQueueTick queue length before pop: {}",
        queue_state.queue_len
    );

    let (vault, _) =
        ephemeral_spl_api::Address::find_program_address(&[queue_state.mint.as_ref()], program_id);

    // Create action callback
    let mut callback_data = [0_u8; 13];
    TransferCallbackArgs {
        amount: queued_transfer.amount,
        group_id: queued_transfer.group_id(),
        flag: queued_transfer.flags,
    }
    .encode_to(&mut callback_data)?;

    let standalone_action_callback_accounts = create_action_callback_accounts(
        tick_accounts.queue_info.address(),
        queued_transfer,
        &vault,
        &queue_state.mint,
    );
    let standalone_action_callback =
        create_action_callback(&standalone_action_callback_accounts, &callback_data);

    let args = ExecuteQueuedTransferArgs {
        amount: queued_transfer.amount,
        client_ref_id: if queued_transfer.client_ref_id != 0 {
            Some(queued_transfer.client_ref_id)
        } else {
            None
        },
        escrow_index: EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX,
        flags: queued_transfer.flags,
    };
    let execute_data =
        ESplInternalInstruction::ExecuteReadyQueuedTransfer.with_data(&args.encode().unwrap());

    let standalone_action_accounts = create_action_accounts(
        &queued_transfer.destination_owner,
        &vault,
        &queue_state.mint,
    );
    let mut standalone_action = execute_queued_transfer_action(
        tick_accounts.queue_info,
        &standalone_action_accounts,
        &execute_data,
    );
    standalone_action.callback = Some(standalone_action_callback);

    invoke_standalone_transfer_action(
        &MagicAccounts {
            queue_info: tick_accounts.queue_info,
            magic_fee_vault_info: tick_accounts.magic_fee_vault_info,
            magic_context_info: tick_accounts.magic_context_info,
            magic_program_info: tick_accounts.magic_program_info,
        },
        &MagicState {
            mint: queue_state.mint,
            queue_bump: queue_state.queue_bump,
            validator: queue_state.validator,
        },
        &[standalone_action],
    )
}

// #[inline(always)]
// fn invoke_queue_standalone_action(
//     tick_accounts: &TickAccounts<'_>,
//     queue_state: &QueueTickState,
//     standalone_actions: &[CallHandler],
// ) -> ProgramResult {
//     let queue_bump_seed = [queue_state.queue_bump];
//     let signer_seeds = [
//         Seed::from(QUEUE_SEED),
//         Seed::from(queue_state.mint.as_ref()),
//         Seed::from(queue_state.validator.as_ref()),
//         Seed::from(&queue_bump_seed),
//     ];
//     let signers = [Signer::from(&signer_seeds)];
//     let mut intent_bundle_data = [0_u8; MAGIC_INTENT_BUNDLE_DATA_LEN];
//     let derived_magic_fee_vault =
//         magic_fee_vault_pda_from_validator(&queue_state.validator.to_bytes().into());
//     require!(
//         derived_magic_fee_vault.to_bytes()
//             == tick_accounts.magic_fee_vault_info.address().to_bytes(),
//         ProgramError::InvalidSeeds
//     );
//
//     MagicIntentBundleBuilder::new(
//         tick_accounts.queue_info.clone(),
//         tick_accounts.magic_context_info.clone(),
//         tick_accounts.magic_program_info.clone(),
//     )
//     .magic_fee_vault(tick_accounts.magic_fee_vault_info.clone())
//     .set_standalone_actions(standalone_actions)
//     .build_and_invoke_signed(&mut intent_bundle_data, &signers)
// }

#[inline(always)]
fn pop_executed_transfer(
    queue_info: &AccountView,
    queued_transfer: QueuedTransfer,
) -> ProgramResult {
    // Note that we delete the queue entry immediately after execution is scheduled (only) and we
    // do not wait for actual payout. It is by design.
    let data = unsafe { queue_info.borrow_unchecked_mut() };
    let popped_transfer = queue_pop_from_data(data)?.ok_or(ProgramError::InvalidAccountData)?;
    require!(
        popped_transfer.task_id == queued_transfer.task_id,
        ProgramError::InvalidAccountData
    );

    debug_log!(
        "ProcessTransferQueueTick group_id: {} task_id: {} client_ref_id: {} sender: {} receiver: {} amount: {}",
        popped_transfer.group_id(),
        popped_transfer.task_id,
        popped_transfer.client_ref_id,
        popped_transfer.source.to_string().as_str(),
        popped_transfer.destination_owner.to_string().as_str(),
        popped_transfer.amount
    );

    Ok(())
}

// fn create_action_accounts(
//     queued_transfer: &QueuedTransfer,
//     vault: &ephemeral_spl_api::Address,
//     mint: &ephemeral_spl_api::Address,
// ) -> [ShortAccountMeta; 9] {
//     let vault_token_account = internal::derive_associated_token_address(vault, mint);
//     let destination_token_account =
//         internal::derive_associated_token_address(&queued_transfer.destination_owner, mint);
//
//     // Note that we initialize CallHandler with 9 accounts only, and then 3 more accounts [source_program,
//     // escrow_authority, escrow_signer] are appended by DLP's CallHandlerV2 instruction, which is
//     // why EXECUTE_READY_QUEUED_TRANSFER receives 12 accounts (not 9).
//     [
//         ShortAccountMeta {
//             pubkey: vault.clone(),
//             is_writable: false,
//         },
//         ShortAccountMeta {
//             pubkey: mint.clone(),
//             is_writable: false,
//         },
//         ShortAccountMeta {
//             pubkey: vault_token_account,
//             is_writable: true,
//         },
//         ShortAccountMeta {
//             pubkey: queued_transfer.destination_owner,
//             is_writable: false,
//         },
//         ShortAccountMeta {
//             pubkey: destination_token_account,
//             is_writable: true,
//         },
//         ShortAccountMeta {
//             pubkey: RENT_PDA,
//             is_writable: true,
//         },
//         ShortAccountMeta {
//             pubkey: TOKEN_PROGRAM_ID,
//             is_writable: false,
//         },
//         ShortAccountMeta {
//             pubkey: ASSOCIATED_TOKEN_PROGRAM_ID,
//             is_writable: false,
//         },
//         ShortAccountMeta {
//             pubkey: SYSTEM_PROGRAM_ID,
//             is_writable: false,
//         },
//     ]
// }

#[inline(never)]
fn create_action_callback_accounts(
    queue_address: &ephemeral_spl_api::Address,
    queued_transfer: &QueuedTransfer,
    vault: &ephemeral_spl_api::Address,
    mint: &ephemeral_spl_api::Address,
) -> [ShortAccountMeta; 11] {
    let vault_token_account = internal::derive_associated_token_address(vault, mint);
    let source_token_account =
        internal::derive_associated_token_address(&queued_transfer.source, mint);
    let (group_receipt_account, _) = derive_group_receipt_id(
        queue_address,
        &queued_transfer.source,
        queued_transfer.group_id(),
    );
    [
        ShortAccountMeta {
            pubkey: CALLBACK_SIGNER,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: group_receipt_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: queue_address.clone(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: vault.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: mint.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: vault_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: queued_transfer.source,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: source_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: TOKEN_PROGRAM_ID,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: MAGIC_VAULT_ID,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: MAGIC_PROGRAM_ID,
            is_writable: false,
        },
    ]
}

fn create_action_callback<'a>(
    accounts: &'a [ShortAccountMeta],
    payload: &'a [u8],
) -> ActionCallback<'a> {
    const CALLBACK_COMPUTE_UNITS: u32 = 100_000;

    ActionCallback {
        destination_program: crate::ID,
        discriminator: &[ESplInternalInstruction::ExecuteTransferCallback as u8],
        payload,
        compute_units: CALLBACK_COMPUTE_UNITS,
        accounts,
    }
}

// fn create_callhandler<'a>(
//     queue_info: &AccountView,
//     action_accounts: &'a [ShortAccountMeta],
//     action_data: &'a [u8],
//     action_callback: ActionCallback<'a>,
// ) -> CallHandler<'a> {
//     CallHandler {
//         destination_program: crate::ID,
//         escrow_authority: queue_info.clone(),
//         args: ActionArgs::new(action_data)
//             .with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
//         compute_units: EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
//         accounts: action_accounts,
//         callback: Some(action_callback),
//     }
// }
