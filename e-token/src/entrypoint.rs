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
            pinocchio::msg!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, instruction_data)
        }
        1 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: InitializeGlobalVault");

            process_initialize_global_vault(accounts, instruction_data)
        }
        2 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: DepositSplTokens");

            process_deposit_spl_tokens(accounts, instruction_data)
        }
        3 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: WithdrawSplTokens");

            process_withdraw_spl_tokens(accounts, instruction_data)
        }
        4 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: DelegateEphemeralAta");

            process_delegate_ephemeral_ata(accounts, instruction_data)
        }
        5 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: UndelegateEphemeralAta");

            process_undelegate_ephemeral_ata(accounts, instruction_data)
        }
        196 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: UndelegationCallback");

            process_undelegation_callback(accounts, instruction_data)
        }
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
