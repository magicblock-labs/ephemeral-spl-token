use ephemeral_rollups_pinocchio::crank::{CrankInstruction, ScheduleCrankCpi};
use ephemeral_spl_api::instruction::internal::PROCESS_TRANSFER_QUEUE_TICK;
use ephemeral_spl_api::state::transfer_queue::{
    queue_crank_task_id_from_data, queue_set_crank_task_id_from_data, queue_views_checked,
    QUEUE_SEED,
};
use pinocchio::instruction::InstructionAccount;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

pub const CRANK_EXECUTION_INTERVAL_MILLIS: i64 = 1000;

const PROCESS_QUEUE_TICK_CRANK_ACCOUNTS: usize = 4;
const SCHEDULE_CRANK_CPI_ACCOUNTS: usize = 4;
const SCHEDULE_CRANK_DATA_LEN: usize =
    4 + 8 + 8 + 8 + 8 + 32 + 8 + (PROCESS_QUEUE_TICK_CRANK_ACCOUNTS * 34) + 8 + 1;

#[inline(always)]
pub fn process_ensure_transfer_queue_crank(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if !instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Expected accounts:
    // 0. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint]
    // 1. [writable, signer] Payer for the recurring crank
    // 2. [writable] Task context account
    // 3. []        Magic program
    let [queue_info, payer_info, task_context_info, magic_program_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if magic_program_info.address() != &ephemeral_rollups_pinocchio::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let program_id = ephemeral_spl_api::program::id_address();
    let mint = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        header.mint
    };

    let (derived_queue, _) =
        ephemeral_spl_api::Address::find_program_address(&[QUEUE_SEED, mint.as_ref()], &program_id);
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&program_id) {
        return Err(ProgramError::IllegalOwner);
    }

    let crank_task_id = derive_queue_crank_task_id(queue_info.address());
    let data = unsafe { queue_info.borrow_unchecked() };
    if let Some(existing_task_id) = queue_crank_task_id_from_data(data)? {
        if existing_task_id == crank_task_id {
            return Ok(());
        }
        return Err(ProgramError::InvalidAccountData);
    }

    let tick_data = [PROCESS_TRANSFER_QUEUE_TICK];
    let tick_accounts = [
        InstructionAccount {
            address: queue_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: payer_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: task_context_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: magic_program_info.address(),
            is_signer: false,
            is_writable: false,
        },
    ];
    let crank_instruction = [CrankInstruction::new(
        ephemeral_spl_api::program::id_address(),
        &tick_accounts,
        &tick_data,
    )];
    let instruction_accounts = [queue_info, task_context_info, magic_program_info];
    let mut crank_data = [0_u8; SCHEDULE_CRANK_DATA_LEN];

    ScheduleCrankCpi::builder(payer_info.clone(), magic_program_info.clone())
        .instruction_accounts(&instruction_accounts)
        .task_id(crank_task_id)
        .execution_interval_millis(CRANK_EXECUTION_INTERVAL_MILLIS)
        .iterations(i64::MAX)
        .instructions(&crank_instruction)
        .build_and_invoke::<SCHEDULE_CRANK_CPI_ACCOUNTS>(&mut crank_data)?;

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    queue_set_crank_task_id_from_data(data, crank_task_id)?;
    Ok(())
}

#[inline(always)]
fn derive_queue_crank_task_id(queue_address: &ephemeral_spl_api::Address) -> i64 {
    let mut acc = 0_u64;
    for chunk in queue_address.as_ref().chunks_exact(8) {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(chunk);
        acc ^= u64::from_le_bytes(bytes);
    }
    acc &= i64::MAX as u64;
    if acc == 0 {
        1
    } else {
        acc as i64
    }
}
