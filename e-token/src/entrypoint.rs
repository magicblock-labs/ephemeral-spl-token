use ephemeral_spl_api::{error::EphemeralSplError, instruction};
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
        0 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, instruction_data)
        }
        1 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeGlobalVault");

            process_initialize_global_vault(accounts, instruction_data)
        }
        2 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DepositSplTokens");

            process_deposit_spl_tokens(accounts, instruction_data)
        }
        3 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: WithdrawSplTokens");

            process_withdraw_spl_tokens(accounts, instruction_data)
        }
        4 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAta");

            process_delegate_ephemeral_ata(accounts, instruction_data)
        }
        5 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAta");

            process_undelegate_ephemeral_ata(accounts, instruction_data)
        }
        6 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CreateEphemeralAtaPermission");

            process_create_ephemeral_ata_permission(accounts, instruction_data)
        }
        7 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateEphemeralAtaPermission");

            process_delegate_ephemeral_ata_permission(accounts, instruction_data)
        }
        8 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateEphemeralAtaPermission");

            process_undelegate_ephemeral_ata_permission(accounts, instruction_data)
        }
        9 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: ResetEphemeralAtaPermission");

            process_reset_ephemeral_ata_permission(accounts, instruction_data)
        }
        10 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseEphemeralAta");

            process_close_ephemeral_ata(accounts, instruction_data)
        }
        11 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: InitializeShuttleEphemeralAta");

            process_initialize_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        13 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateShuttleEphemeralAta");

            process_delegate_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        instruction::UNDELEGATE_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegateShuttleEphemeralAta");

            process_undelegate_and_close_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        15 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: MergeShuttleIntoEphemeralAta");

            process_merge_shuttle_into_ephemeral_ata(accounts, instruction_data)
        }
        instruction::DELEGATE_AND_MERGE_SHUTTLE_EPHEMERAL_ATA => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: DelegateAndMergeShuttleEphemeralAta");

            process_delegate_and_merge_shuttle_ephemeral_ata(accounts, instruction_data)
        }
        196 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegationCallback");

            process_undelegation_callback(accounts, instruction_data)
        }
        197 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: CloseShuttleAtaIntentV2");

            process_close_shuttle_ata_intent_v2(accounts, instruction_data)
        }
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
