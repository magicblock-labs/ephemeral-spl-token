use ephemeral_spl_api::error::EphemeralSplError;
use ephemeral_spl_api::instruction::{self, internal};
use {
    crate::processor::*,
    core::{mem::MaybeUninit, slice::from_raw_parts},
    pinocchio::{
        entrypoint::deserialize, error::ProgramError, no_allocator, nostd_panic_handler,
        AccountView, ProgramResult, MAX_TX_ACCOUNTS, SUCCESS,
    },
};

// Do not allocate memory.
no_allocator!();
// Use the no_std panic handler.
nostd_panic_handler!();

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
fn log_error(_error: &ProgramError) {
    pinocchio_log::log!("Program error");
}

/// Process an instruction.
#[inline(always)]
pub fn process_instruction(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let result = inner_process_instruction(accounts, instruction_data);
    result.inspect_err(log_error)
}

/// Process an instruction.
#[inline(always)]
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
        instruction::UNDELEGATE_SHUTTLE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateShuttleEphemeralAta");

            process_undelegate_and_close_shuttle_ephemeral_ata(accounts, instruction_data)
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
        instruction::INITIALIZE_FEES_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeFeesPda");

            process_initialize_fees_pda(accounts, instruction_data)
        }
        instruction::DELEGATE_FEES_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateFeesPda");

            process_delegate_fees_pda(accounts, instruction_data)
        }
        instruction::COMMIT_FEES_PDA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CommitFeesPda");

            process_commit_fees_pda(accounts, instruction_data)
        }
        internal::UNDELEGATION_CALLBACK => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegationCallback");

            process_undelegation_callback(accounts, instruction_data)
        }
        internal::CLOSE_SHUTTLE_ATA_INTENT_V2 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseShuttleAtaIntentV2");

            process_close_shuttle_ata_intent_v2(accounts, instruction_data)
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
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
