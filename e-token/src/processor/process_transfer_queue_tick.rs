use crate::processor::rent_pda::derive_rent_pda;
use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_rollups_pinocchio::spl::consts::TOKEN_PROGRAM_ID;
use ephemeral_spl_api::instruction::internal::EXECUTE_READY_QUEUED_TRANSFER;
use ephemeral_spl_api::state::transfer_queue::{
    queue_peek_from_data, queue_pop_from_data, queue_views_checked, QUEUE_SEED,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::clock::Clock;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;
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
    #[cfg(not(feature = "logging"))]
    let _ = queue_len;

    let (vault, _) =
        ephemeral_spl_api::Address::find_program_address(&[mint.as_ref()], &program_id);
    let vault_token_account = derive_associated_token_address(&vault, &mint);
    let destination_token_account =
        derive_associated_token_address(&queued_transfer.destination_owner, &mint);
    let (rent_pda, _) = derive_rent_pda();
    let mut execute_data = [0_u8; 11];
    execute_data[0] = EXECUTE_READY_QUEUED_TRANSFER;
    execute_data[1] = EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX;
    execute_data[2..10].copy_from_slice(&queued_transfer.amount.to_le_bytes());
    execute_data[10] = queued_transfer.flags;
    let execute_accounts = [
        ShortAccountMeta {
            pubkey: vault,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: mint,
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
            pubkey: rent_pda,
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
        destination_program: ephemeral_spl_api::program::id_address(),
        escrow_authority: queue_info.clone(),
        args: ActionArgs::new(&execute_data)
            .with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
        compute_units: EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
        accounts: &execute_accounts,
    }];
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
    pinocchio_log::log!(
        "ProcessTransferQueueTick queue length after pop: {}",
        queue_len - 1
    );

    Ok(())
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
