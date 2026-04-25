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
        if instruction_data[0] < ESplInternalInstruction::UndelegationCallback.discriminator() {
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
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeGlobalVault => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeGlobalVault");

            process_initialize_global_vault(accounts, data)
        }
        ESplInstruction::DepositSplTokens => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DepositSplTokens");

            process_deposit_spl_tokens(accounts, data)
        }
        ESplInstruction::WithdrawSplTokens => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: WithdrawSplTokens");

            process_withdraw_spl_tokens(accounts, data)
        }
        ESplInstruction::DelegateEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAta");

            process_delegate_ephemeral_ata(accounts, data)
        }
        ESplInstruction::UndelegateEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAta");

            process_undelegate_ephemeral_ata(accounts, data)
        }
        ESplInstruction::CreateEphemeralAtaPermission => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CreateEphemeralAtaPermission");

            process_create_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::DelegateEphemeralAtaPermission => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAtaPermission");

            process_delegate_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::UndelegateEphemeralAtaPermission => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAtaPermission");

            process_undelegate_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::ResetEphemeralAtaPermission => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ResetEphemeralAtaPermission");

            process_reset_ephemeral_ata_permission(accounts, data)
        }
        ESplInstruction::CloseEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseEphemeralAta");

            process_close_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeShuttleEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeShuttleEphemeralAta");

            process_initialize_shuttle_ephemeral_ata(accounts, data)
        }
        ESplInstruction::InitializeTransferQueue => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeTransferQueue");

            process_initialize_transfer_queue(accounts, data)
        }
        ESplInstruction::DelegateShuttleEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateShuttleEphemeralAta");

            process_delegate_shuttle_ephemeral_ata(accounts, data)
        }
        ESplInstruction::SetupAndDelegateShuttleEphemeralAtaWithMerge => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SetupAndDelegateShuttleEphemeralAtaWithMerge");

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(accounts, data)
        }
        ESplInstruction::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!(
                "Instruction: DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer"
            );

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
                accounts, data,
            )
        }
        ESplInstruction::WithdrawThroughDelegatedShuttleWithMerge => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: WithdrawThroughDelegatedShuttleWithMerge");

            process_withdraw_through_delegated_shuttle_with_merge(accounts, data)
        }
        ESplInstruction::UndelegateAndCloseShuttleToOwner => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateAndCloseShuttleToOwner");

            process_undelegate_and_close_shuttle_to_owner(accounts, data)
        }
        ESplInstruction::MergeShuttleIntoEphemeralAta => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: MergeShuttleIntoEphemeralAta");

            process_merge_shuttle_into_ephemeral_ata(accounts, data)
        }
        ESplInstruction::DepositAndQueueTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DepositAndQueueTransfer");

            process_deposit_and_queue_transfer(accounts, data)
        }
        ESplInstruction::EnsureTransferQueueCrank => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: EnsureTransferQueueCrank");

            process_ensure_transfer_queue_crank(accounts, data)
        }
        ESplInstruction::DelegateTransferQueue => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateTransferQueue");

            process_delegate_transfer_queue(accounts, data)
        }
        ESplInstruction::SponsoredLamportsTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SponsoredLamportsTransfer");

            process_sponsored_lamports_transfer(accounts, data)
        }
        ESplInstruction::InitializeRentPda => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeRentPda");

            process_initialize_rent_pda(accounts, data)
        }
        ESplInstruction::AllocateTransferQueue => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: AllocateTransferQueue");

            process_allocate_transfer_queue(accounts, data)
        }
        ESplInstruction::ProcessPendingTransferQueueRefill => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ProcessPendingTransferQueueRefill");

            process_pending_transfer_queue_refill(accounts, data)
        }
        ESplInstruction::ProcessScheduledPrivateTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ProcessScheduledPrivateTransfer");

            process_scheduled_private_transfer(accounts, instruction_data)
        }
        ESplInstruction::SchedulePrivateTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SchedulePrivateTransfer");

            process_schedule_private_transfer(accounts, instruction_data)
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
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegationCallback");
            process_undelegation_callback(accounts, data)
        }
        ESplInternalInstruction::SettleAndCloseShuttleIntent => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SettleAndCloseShuttleIntent");

            process_close_shuttle_ata_intent(accounts, data)
        }
        ESplInternalInstruction::ExecuteReadyQueuedTransfer => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ExecuteReadyQueuedTransfer");

            process_execute_ready_queued_transfer(accounts, data)
        }
        ESplInternalInstruction::ProcessTransferQueueTick => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ProcessTransferQueueTick");

            process_transfer_queue_tick(accounts, data)
        }
        ESplInternalInstruction::TransferLamportsPda => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: TransferLamportsPda");

            process_transfer_lamports_pda(accounts, data)
        }
        ESplInternalInstruction::UndelegateLamportsPda => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateLamportsPda");

            process_undelegate_lamports_pda(accounts, data)
        }
        ESplInternalInstruction::CloseLamportsPdaIntent => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseLamportsPdaIntent");

            process_close_lamports_pda_intent(accounts, data)
        }
        ESplInternalInstruction::MarkTransferQueueRefillPending => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: MarkTransferQueueRefillPending");

            process_mark_transfer_queue_refill_pending(accounts, data)
        }
    }
}
