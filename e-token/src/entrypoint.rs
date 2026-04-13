use crate::processor::initialize_group_receipt::process_initialize_group_receipt;
use ephemeral_spl_api::error::EphemeralSplError;
use ephemeral_spl_api::instruction::{self, internal};
use {
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
    let result = inner_process_instruction(accounts, instruction_data);
    result.inspect_err(log_error)
}

/// Process an instruction.
#[inline(never)]
pub(crate) fn inner_process_instruction(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [discriminator, instruction_data @ ..] = instruction_data else {
        return Err(EphemeralSplError::InvalidInstruction.into());
    };

    match *discriminator {
        instruction::INITIALIZE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, instruction_data)
        }
        instruction::INITIALIZE_GLOBAL_VAULT => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeGlobalVault");

            process_initialize_global_vault(accounts, instruction_data)
        }
        instruction::DEPOSIT_SPL_TOKENS => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DepositSplTokens");

            process_deposit_spl_tokens(accounts, instruction_data)
        }
        instruction::WITHDRAW_SPL_TOKENS => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: WithdrawSplTokens");

            process_withdraw_spl_tokens(accounts, instruction_data)
        }
        instruction::DELEGATE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAta");

            process_delegate_ephemeral_ata(accounts, instruction_data)
        }
        instruction::UNDELEGATE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAta");

            process_undelegate_ephemeral_ata(accounts, instruction_data)
        }
        instruction::CREATE_EPHEMERAL_ATA_PERMISSION => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CreateEphemeralAtaPermission");

            process_create_ephemeral_ata_permission(accounts, instruction_data)
        }
        instruction::DELEGATE_EPHEMERAL_ATA_PERMISSION => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAtaPermission");

            process_delegate_ephemeral_ata_permission(accounts, instruction_data)
        }
        instruction::UNDELEGATE_EPHEMERAL_ATA_PERMISSION => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAtaPermission");

            process_undelegate_ephemeral_ata_permission(accounts, instruction_data)
        }
        instruction::RESET_EPHEMERAL_ATA_PERMISSION => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ResetEphemeralAtaPermission");

            process_reset_ephemeral_ata_permission(accounts, instruction_data)
        }
        instruction::CLOSE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseEphemeralAta");

            process_close_ephemeral_ata(accounts, instruction_data)
        }
        instruction::INITIALIZE_SHUTTLE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeShuttleEphemeralAta");

            process_initialize_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        instruction::INITIALIZE_TRANSFER_QUEUE => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeTransferQueue");

            process_initialize_transfer_queue(accounts, instruction_data)
        }
        instruction::DELEGATE_SHUTTLE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateShuttleEphemeralAta");

            process_delegate_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        instruction::SETUP_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SetupAndDelegateShuttleEphemeralAtaWithMerge");

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(
                accounts,
                instruction_data,
            )
        }
        instruction::DEPOSIT_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE_AND_PRIVATE_TRANSFER => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!(
                "Instruction: DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer"
            );

            process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
                accounts,
                instruction_data,
            )
        }
        instruction::WITHDRAW_THROUGH_DELEGATED_SHUTTLE_WITH_MERGE => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: WithdrawThroughDelegatedShuttleWithMerge");

            process_withdraw_through_delegated_shuttle_with_merge(accounts, instruction_data)
        }
        instruction::UNDELEGATE_AND_CLOSE_SHUTTLE_TO_OWNER => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateAndCloseShuttleToOwner");

            process_undelegate_and_close_shuttle_to_owner(accounts, instruction_data)
        }
        instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: MergeShuttleIntoEphemeralAta");

            process_merge_shuttle_into_ephemeral_ata(accounts, instruction_data)
        }
        instruction::DEPOSIT_AND_QUEUE_TRANSFER => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DepositAndQueueTransfer");

            process_deposit_and_queue_transfer(accounts, instruction_data)
        }
        instruction::ENSURE_TRANSFER_QUEUE_CRANK => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: EnsureTransferQueueCrank");

            process_ensure_transfer_queue_crank(accounts, instruction_data)
        }
        instruction::DELEGATE_TRANSFER_QUEUE => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateTransferQueue");

            process_delegate_transfer_queue(accounts, instruction_data)
        }
        instruction::SPONSORED_LAMPORTS_TRANSFER => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SponsoredLamportsTransfer");

            process_sponsored_lamports_transfer(accounts, instruction_data)
        }
        instruction::INITIALIZE_RENT_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeRentPda");

            process_initialize_rent_pda(accounts, instruction_data)
        }
        instruction::ALLOCATE_TRANSFER_QUEUE => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: AllocateTransferQueue");

            process_allocate_transfer_queue(accounts, instruction_data)
        }
        instruction::PROCESS_PENDING_TRANSFER_QUEUE_REFILL => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ProcessPendingTransferQueueRefill");

            process_pending_transfer_queue_refill(accounts, instruction_data)
        }
        internal::UNDELEGATION_CALLBACK => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegationCallback");

            process_undelegation_callback(accounts, instruction_data)
        }
        internal::SETTLE_AND_CLOSE_SHUTTLE_INTENT => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: SettleAndCloseShuttleIntent");

            process_close_shuttle_ata_intent(accounts, instruction_data)
        }
        internal::EXECUTE_READY_QUEUED_TRANSFER => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ExecuteReadyQueuedTransfer");

            process_execute_ready_queued_transfer(accounts, instruction_data)
        }
        internal::PROCESS_TRANSFER_QUEUE_TICK => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ProcessTransferQueueTick");

            process_transfer_queue_tick(accounts, instruction_data)
        }
        internal::TRANSFER_LAMPORTS_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: TransferLamportsPda");

            process_transfer_lamports_pda(accounts, instruction_data)
        }
        internal::UNDELEGATE_LAMPORTS_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateLamportsPda");

            process_undelegate_lamports_pda(accounts, instruction_data)
        }
        internal::CLOSE_LAMPORTS_PDA_INTENT => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseLamportsPdaIntent");

            process_close_lamports_pda_intent(accounts, instruction_data)
        }
        internal::MARK_TRANSFER_QUEUE_REFILL_PENDING => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: MarkTransferQueueRefillPending");

            process_mark_transfer_queue_refill_pending(accounts, instruction_data)
        }
        internal::EXECUTE_TRANSFER_CALLBACK => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ExecuteTransferCallback");

            process_execute_transfer_callback(accounts, instruction_data)
        }
        internal::INITIALIZE_GROUP_RECEIPT => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeGroupReceipt");

            process_initialize_group_receipt(accounts, instruction_data)
        }
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
