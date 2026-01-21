use ephemeral_spl_api::error::EphemeralSplError;
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
    const UNINIT: MaybeUninit<AccountView> = MaybeUninit::<AccountView>::uninit();
    let mut accounts = [UNINIT; { MAX_TX_ACCOUNTS }];

    let (_, count, instruction_data) = deserialize::<MAX_TX_ACCOUNTS>(input, &mut accounts);

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
            pinocchio_log::log!("Instruction: CloseEphemeralAtaPermission");

            process_close_ephemeral_ata_permission(accounts, instruction_data)
        }
        10 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UpdateEphemeralAtaPermission");

            process_update_ephemeral_ata_permission(accounts, instruction_data)
        }
        196 => {
            #[cfg(feature = "logging")]
            pinocchio_log::log!("Instruction: UndelegationCallback");

            process_undelegation_callback(accounts, instruction_data)
        }
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
