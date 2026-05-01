use ephemeral_spl_api::debug_log;
use ephemeral_spl_api::instruction::ESplInstruction;
use ephemeral_spl_api::{error::EphemeralSplError, require};

use {
    crate::instruction::ESplInternalInstruction,
    crate::processor::*,
    core::{mem::MaybeUninit, slice::from_raw_parts},
    pinocchio::{
        default_allocator, default_panic_handler, entrypoint::deserialize, error::ProgramError,
        AccountView, ProgramResult, MAX_TX_ACCOUNTS, SUCCESS,
    },
};

default_allocator!();
default_panic_handler!();

#[no_mangle]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    const MAX_PROGRAM_ACCOUNTS: usize = MAX_TX_ACCOUNTS;
    const UNINIT: MaybeUninit<AccountView> = MaybeUninit::<AccountView>::uninit();
    let mut accounts = [UNINIT; { MAX_PROGRAM_ACCOUNTS }];

    let (_, count, instruction_data) = deserialize::<MAX_PROGRAM_ACCOUNTS>(input, &mut accounts);

    match process_instruction(
        from_raw_parts(accounts.as_ptr() as _, count),
        instruction_data,
    ) {
        Ok(()) => SUCCESS,
        Err(error) => error.into(),
    }
}

/// Log an error.
#[cold]
fn log_error(error: &ProgramError) {
    pinocchio_log::log!(
        "Instruction failed with: {}",
        error.to_str::<EphemeralSplError>()
    );
}

/// Process an instruction.
#[inline(never)]
pub fn process_instruction(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    require!(
        !instruction_data.is_empty(),
        ProgramError::InvalidInstructionData
    );

    let result = {
        // UndelegationCallback is the first internal type, so anything less than that is public
        // instruction
        if instruction_data[0] < ESplInternalInstruction::UndelegationCallback.value() {
            process_public_instruction(accounts, instruction_data)
        } else {
            process_internal_instruction(accounts, instruction_data)
        }
    };
    result.inspect_err(log_error)
}

/// Process public instruction
#[inline(never)]
fn process_public_instruction(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let (discriminator, data) = instruction_data.split_at(1);

    match ESplInstruction::try_from(discriminator[0])
        .map_err(|_| EphemeralSplError::InstructionNotFound)?
    {
        ESplInstruction::InitializeEphemeralAta => {
            debug_log!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeGlobalVault => {
            debug_log!("Instruction: InitializeGlobalVault");

            process_initialize_global_vault(accounts, data)
        }
        ESplInstruction::DepositSplTokens => {
            debug_log!("Instruction: DepositSplTokens");

            process_deposit_spl_tokens(accounts, data)
        }
        ESplInstruction::WithdrawSplTokens => {
            debug_log!("Instruction: WithdrawSplTokens");

            process_withdraw_spl_tokens(accounts, data)
        }
        ESplInstruction::DelegateEphemeralAta => {
            debug_log!("Instruction: DelegateEphemeralAta");

            process_delegate_ephemeral_ata(accounts, data)
        }
        ESplInstruction::UndelegateEphemeralAta => {
            debug_log!("Instruction: UndelegateEphemeralAta");

            process_undelegate_ephemeral_ata(accounts, data)
        }
        ESplInstruction::CreateEphemeralAtaPermission => {
            debug_log!("Instruction: CreateEphemeralAtaPermission");

            process_create_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::DelegateEphemeralAtaPermission => {
            debug_log!("Instruction: DelegateEphemeralAtaPermission");

            process_delegate_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::UndelegateEphemeralAtaPermission => {
            debug_log!("Instruction: UndelegateEphemeralAtaPermission");

            process_undelegate_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::ResetEphemeralAtaPermission => {
            debug_log!("Instruction: ResetEphemeralAtaPermission");

            process_reset_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::CloseEphemeralAta => {
            debug_log!("Instruction: CloseEphemeralAta");

            process_close_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeShuttleEphemeralAta => {
            debug_log!("Instruction: InitializeShuttleEphemeralAta");

            process_initialize_shuttle_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeTransferQueue => {
            debug_log!("Instruction: InitializeTransferQueue");

            process_initialize_transfer_queue(accounts, data)
        }
        ESplInstruction::DelegateShuttleEphemeralAta => {
            debug_log!("Instruction: DelegateShuttleEphemeralAta");

            process_delegate_shuttle_ephemeral_ata(accounts, data)
        }
        ESplInstruction::SetupAndDelegateShuttleEphemeralAtaWithMerge => {
            debug_log!("Instruction: SetupAndDelegateShuttleEphemeralAtaWithMerge");

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(accounts, data)
        }
        ESplInstruction::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer => {
            debug_log!(
                "Instruction: DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer"
            );

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
                accounts, data,
            )
        }
        ESplInstruction::WithdrawThroughDelegatedShuttleWithMerge => {
            debug_log!("Instruction: WithdrawThroughDelegatedShuttleWithMerge");

            process_withdraw_through_delegated_shuttle_with_merge(accounts, data)
        }
        ESplInstruction::UndelegateAndCloseShuttleToOwner => {
            debug_log!("Instruction: UndelegateAndCloseShuttleToOwner");

            process_undelegate_and_close_shuttle_to_owner(accounts, data)
        }
        ESplInstruction::MergeShuttleIntoEphemeralAta => {
            debug_log!("Instruction: MergeShuttleIntoEphemeralAta");

            process_merge_shuttle_into_ephemeral_ata(accounts, data)
        }
        ESplInstruction::DepositAndQueueTransfer => {
            debug_log!("Instruction: DepositAndQueueTransfer");

            process_deposit_and_queue_transfer(accounts, data)
        }
        ESplInstruction::EnsureTransferQueueCrank => {
            debug_log!("Instruction: EnsureTransferQueueCrank");

            process_ensure_transfer_queue_crank(accounts, data)
        }
        ESplInstruction::DelegateTransferQueue => {
            debug_log!("Instruction: DelegateTransferQueue");

            process_delegate_transfer_queue(accounts, data)
        }
        ESplInstruction::SponsoredLamportsTransfer => {
            debug_log!("Instruction: SponsoredLamportsTransfer");

            process_sponsored_lamports_transfer(accounts, data)
        }
        ESplInstruction::InitializeRentPda => {
            debug_log!("Instruction: InitializeRentPda");

            process_initialize_rent_pda(accounts, data)
        }
        ESplInstruction::AllocateTransferQueue => {
            debug_log!("Instruction: AllocateTransferQueue");

            process_allocate_transfer_queue(accounts, data)
        }
        ESplInstruction::ExecutePendingTransferQueueRefill => {
            debug_log!("Instruction: ExecutePendingTransferQueueRefill");

            process_execute_pending_transfer_queue_refill(accounts, data)
        }
        ESplInstruction::ExecuteScheduledPrivateTransfer => {
            debug_log!("Instruction: ExecuteScheduledPrivateTransfer");

            process_execute_scheduled_private_transfer(accounts, data)
        }
        ESplInstruction::SchedulePrivateTransfer => {
            debug_log!("Instruction: SchedulePrivateTransfer");

            process_schedule_private_transfer(accounts, data)
        }
    }
}

/// Process internal instruction
#[inline(never)]
fn process_internal_instruction(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let (discriminator, data) = instruction_data.split_at(1);

    match ESplInternalInstruction::try_from(discriminator[0])
        .map_err(|_| EphemeralSplError::InstructionNotFound)?
    {
        ESplInternalInstruction::UndelegationCallback => {
            debug_log!("Instruction: UndelegationCallback");
            process_undelegation_callback(accounts, data)
        }
        ESplInternalInstruction::SettleAndCloseShuttleIntent => {
            debug_log!("Instruction: SettleAndCloseShuttleIntent");

            process_close_shuttle_ata_intent(accounts, data)
        }
        ESplInternalInstruction::ExecuteReadyQueuedTransfer => {
            debug_log!("Instruction: ExecuteReadyQueuedTransfer");

            process_execute_ready_queued_transfer(accounts, data)
        }
        ESplInternalInstruction::ProcessTransferQueueTick => {
            debug_log!("Instruction: ProcessTransferQueueTick");

            process_transfer_queue_tick(accounts, data)
        }
        ESplInternalInstruction::TransferLamportsPda => {
            debug_log!("Instruction: TransferLamportsPda");

            process_transfer_lamports_pda(accounts, data)
        }
        ESplInternalInstruction::UndelegateLamportsPda => {
            debug_log!("Instruction: UndelegateLamportsPda");

            process_undelegate_lamports_pda(accounts, data)
        }
        ESplInternalInstruction::CloseLamportsPdaIntent => {
            debug_log!("Instruction: CloseLamportsPdaIntent");

            process_close_lamports_pda_intent(accounts, data)
        }
        ESplInternalInstruction::MarkTransferQueueRefillPending => {
            debug_log!("Instruction: MarkTransferQueueRefillPending");

            process_mark_transfer_queue_refill_pending(accounts, data)
        }
    }
}
