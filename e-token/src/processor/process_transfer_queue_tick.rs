#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, ActionCallback, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_rollups_pinocchio::spl::consts::TOKEN_PROGRAM_ID;
use ephemeral_spl_api::instruction::internal::{
    EXECUTE_READY_QUEUED_TRANSFER, EXECUTE_TRANSFER_CALLBACK,
};
use ephemeral_spl_api::state::transfer_queue::{
    queue_peek_from_data, queue_pop_from_data, queue_views_checked, QueuedTransfer, QUEUE_SEED,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;

use crate::processor::rent_pda::RENT_PDA;
pub(crate) const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;

const ASSOCIATED_TOKEN_PROGRAM_ID: ephemeral_spl_api::Address =
    pinocchio_associated_token_account::ID;
const EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS: u32 = 140_000;
const MAGIC_INTENT_BUNDLE_DATA_LEN: usize = 512;
const MILLIS_PER_SECOND: i64 = 1_000;

#[inline(always)]
pub fn process_transfer_queue_tick(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

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

    let program_id = ephemeral_spl_api::program::id_address();
    let clock = Clock::get()?;
    let (mint, queue_bump, queue_len, queued_transfer, validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        let mint = header.mint;
        let queue_len = header.length as usize;
        let now = clock
            .unix_timestamp
            .checked_mul(MILLIS_PER_SECOND)
            .ok_or(ProgramError::InvalidInstructionData)?;

        let (derived_queue, queue_bump) = ephemeral_spl_api::Address::find_program_address(
            &[QUEUE_SEED, mint.as_ref(), header.validator.as_ref()],
            &program_id,
        );
        if derived_queue != *queue_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }
        if !queue_info.owned_by(&program_id) {
            return Err(ProgramError::IllegalOwner);
        }

        let Some(next) = queue_peek_from_data(data)? else {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("ProcessTransferQueueTick queue length: {}", queue_len);
            return Ok(());
        };
        if next.ready_at > now {
            #[cfg(feature = "logging")]
            pinocchio_log::log!(
                "ProcessTransferQueueTick queue length: {} (next not ready)",
                queue_len
            );
            return Ok(());
        }

        #[cfg(feature = "logging")]
        pinocchio_log::log!(
            "ProcessTransferQueueTick queue length before pop: {}",
            queue_len
        );

        (mint, queue_bump, queue_len, next, header.validator)
    };
    let _ = queue_len;

    let (vault, _) = Address::find_program_address(&[mint.as_ref()], &program_id);

    let amount_bytes: [u8; 8] = queued_transfer.amount.to_le_bytes();

    // Create action callback
    let mut callback_data = [0_u8; 10];
    callback_data[0..8].copy_from_slice(&amount_bytes);
    callback_data[9] = queued_transfer.flags;

    let standalone_action_callback_accounts =
        create_action_callback_accounts(&validator, &queued_transfer, &vault, &mint);
    let standalone_action_callback =
        create_action_callback(&standalone_action_callback_accounts, &callback_data);

    // Create action with callback
    let mut execute_data = [0_u8; 19];
    execute_data[0] = EXECUTE_READY_QUEUED_TRANSFER;
    execute_data[1] = EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX;
    execute_data[2..10].copy_from_slice(&amount_bytes);
    execute_data[10] = queued_transfer.flags;
    let execute_data_len = if queued_transfer.client_ref_id != 0 {
        execute_data[11..19].copy_from_slice(&queued_transfer.client_ref_id.to_le_bytes());
        19
    } else {
        11
    };
    
    let standalone_action_accounts = create_action_accounts(&queued_transfer, &vault, &mint);
    let standalone_actions = [create_callhandler(
        &queue_info,
        &standalone_action_accounts,
        &execute_data[..execute_data_len],
        standalone_action_callback,
    )];

    let mut intent_bundle_data = [0_u8; MAGIC_INTENT_BUNDLE_DATA_LEN];
    let queue_bump_seed = [queue_bump];
    let signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(mint.as_ref()),
        Seed::from(validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let signer = Signer::from(&signer_seeds);
    let derived_magic_fee_vault = magic_fee_vault_pda_from_validator(&validator.to_bytes().into());
    if derived_magic_fee_vault.to_bytes() != magic_fee_vault_info.address().to_bytes() {
        return Err(ProgramError::InvalidSeeds);
    }

    MagicIntentBundleBuilder::new(
        queue_info.clone(),
        magic_context_info.clone(),
        magic_program_info.clone(),
    )
    .magic_fee_vault(magic_fee_vault_info.clone())
    .set_standalone_actions(&standalone_actions)
    .build_and_invoke_signed(&mut intent_bundle_data, &[signer])?;

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

fn create_action_accounts(
    queued_transfer: &QueuedTransfer,
    vault: &Address,
    mint: &Address,
) -> [ShortAccountMeta; 9] {
    let vault_token_account = derive_associated_token_address(vault, mint);
    let destination_token_account =
        derive_associated_token_address(&queued_transfer.destination_owner, mint);

    let action_accounts = [
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

    action_accounts
}

fn create_action_callback_accounts(
    validator: &Address,
    queued_transfer: &QueuedTransfer,
    vault: &Address,
    mint: &Address,
) -> [ShortAccountMeta; 5] {
    let vault_token_account = derive_associated_token_address(vault, mint);
    let source_token_account = derive_associated_token_address(&queued_transfer.source, mint);
    let callback_accounts = [
        // ShortAccountMeta {
        //     pubkey: validator.clone(),
        //     is_writable: false,
        // },
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
            pubkey: source_token_account,
            is_writable: false, // TODO(edwin): true if delegated
        },
        ShortAccountMeta {
            pubkey: TOKEN_PROGRAM_ID,
            is_writable: false,
        },
    ];

    callback_accounts
}

fn create_action_callback<'a>(
    accounts: &'a [ShortAccountMeta],
    payload: &'a [u8],
) -> ActionCallback<'a> {
    const CALLBACK_COMPUTE_UNITS: u32 = 100_000;

    ActionCallback {
        destination_program: ephemeral_spl_api::program::id_address(),
        discriminator: &[EXECUTE_TRANSFER_CALLBACK],
        payload,
        compute_units: CALLBACK_COMPUTE_UNITS,
        accounts,
    }
}

fn create_callhandler<'a>(
    queue_info: &AccountView,
    action_accounts: &'a [ShortAccountMeta],
    action_data: &'a [u8],
    action_callback: ActionCallback<'a>,
) -> CallHandler<'a> {
    let action = CallHandler {
        destination_program: ephemeral_spl_api::program::id_address(),
        escrow_authority: queue_info.clone(),
        args: ActionArgs::new(action_data)
            .with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
        compute_units: EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
        accounts: action_accounts,
        callback: Some(action_callback),
    };

    action
}

#[inline(always)]
pub(crate) fn derive_associated_token_address(
    wallet: &ephemeral_spl_api::Address,
    mint: &ephemeral_spl_api::Address,
) -> ephemeral_spl_api::Address {
    ephemeral_spl_api::Address::find_program_address(
        &[wallet.as_ref(), TOKEN_PROGRAM_ID.as_ref(), mint.as_ref()],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}
