use ephemeral_spl_api::error::EphemeralSplError;
use {
    crate::processor::*,
    core::{
        mem::MaybeUninit,
        slice::from_raw_parts,
    },
    pinocchio::{
        account_info::AccountInfo,
        entrypoint::deserialize
        ,
        log::sol_log,
        no_allocator, nostd_panic_handler,
        program_error::{ProgramError, ToStr},
        ProgramResult, MAX_TX_ACCOUNTS, SUCCESS,
    },
};

// Do not allocate memory.
no_allocator!();
// Use the no_std panic handler.
nostd_panic_handler!();

#[no_mangle]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    const UNINIT: MaybeUninit<AccountInfo> = MaybeUninit::<AccountInfo>::uninit();
    let mut accounts = [UNINIT; { MAX_TX_ACCOUNTS }];

    let (_, count, instruction_data) = deserialize(input, &mut accounts);

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
    sol_log(error.to_str::<EphemeralSplError>());
}

/// Process an instruction.
#[inline(always)]
pub fn process_instruction(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    let result = inner_process_instruction(accounts, instruction_data);
    result.inspect_err(log_error)
}

/// Process an instruction.
#[inline(always)]
pub(crate) fn inner_process_instruction(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let [discriminator, instruction_data @ ..] = instruction_data else {
        return Err(EphemeralSplError::InvalidInstruction.into());
    };

    match *discriminator {
        // 0 - InitializeMint
        0 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: InitializeMint");

            process_initialize_ephemeral_ata(accounts, instruction_data)
        }
        // 1 - InitializeEphemeralAta
        1 => {
            #[cfg(feature = "logging")]
            pinocchio::msg!("Instruction: InitializeEphemeralAta");

            process_initialize_ephemeral_ata(accounts, instruction_data)
        }
        _ => Err(EphemeralSplError::InvalidInstruction.into()),
    }
}
