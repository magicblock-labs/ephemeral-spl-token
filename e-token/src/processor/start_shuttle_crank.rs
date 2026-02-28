use ephemeral_rollups_pinocchio::crank::{CrankInstruction, ScheduleCrankArgs, ScheduleCrankCpi};
use ephemeral_spl_api::{
    constants::SHUTTLE_CRANK_INTERVAL_MILLIS, instruction, state::transfer_queue::TransferQueue,
};
use pinocchio::{cpi::Signer, instruction::InstructionAccount};

use {
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

#[inline(always)]
pub fn process_start_shuttle_crank(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = StartShuttleCrankArgs::try_from_bytes(instruction_data)?;

    let [
        mint_info, // readonly
        queue_info, // writable, PDA [mint]
        _queue_ata_info, // writable, ATA for [depositor, mint]
        _shuttle_info, // writable, shuttle PDA
        _shuttle_ata_info, // writable, ATA for [shuttle, mint]
        _shuttle_eata_info, // readonly, EATA for [shuttle, mint]
        _magic_context_info, // writable, magic context PDA
        magic_program_info, // [] magic program ID
        _token_program_info, // []
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let queue = unsafe { load_mut_unchecked::<TransferQueue>(queue_info.borrow_unchecked_mut())? };
    let queue_pda = TransferQueue::create_address(&mint_info.address(), &[queue.bump])?;
    if &queue_pda != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if queue.shuttle_crank_id.is_some() {
        return Ok(());
    }

    queue.shuttle_crank_id = Some(args.crank_id);

    let bump = [queue.bump];
    let seed = TransferQueue::signer_seeds(&mint_info.address(), &bump);
    let signer = Signer::from(&seed);

    let instruction_accounts: [InstructionAccount; 9] =
        core::array::from_fn(|i| InstructionAccount {
            address: accounts[i].address(),
            is_writable: accounts[i].is_writable(),
            is_signer: accounts[i].is_signer(),
        });

    let args = ScheduleCrankArgs {
        task_id: args.crank_id,
        execution_interval_millis: SHUTTLE_CRANK_INTERVAL_MILLIS,
        iterations: i64::MAX,
        instructions: &[CrankInstruction {
            program_id: ephemeral_spl_api::program::ID,
            accounts: unsafe {
                core::slice::from_raw_parts(
                    instruction_accounts.as_ptr() as *const InstructionAccount,
                    accounts.len(),
                )
            },
            data: &[instruction::PROCESS_TRANSFERS, args.shuttle_eata_bump],
        }],
    };

    let cpi_accounts: [&AccountView; 9] = core::array::from_fn(|i| &accounts[i]);
    ScheduleCrankCpi {
        payer: queue_info.clone(),
        magic_program: magic_program_info.clone(),
        instruction_accounts: unsafe {
            core::slice::from_raw_parts(
                cpi_accounts.as_ptr() as *const &AccountView,
                accounts.len(),
            )
        },
        args,
    }
    .invoke_signed(&[signer])?;

    Ok(())
}

#[repr(C)]
pub struct StartShuttleCrankArgs {
    crank_id: i64,
    shuttle_eata_bump: u8,
}

impl StartShuttleCrankArgs {
    pub fn try_from_bytes(bytes: &[u8]) -> Result<&StartShuttleCrankArgs, ProgramError> {
        if bytes.len() != 9 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(unsafe { &*(bytes.as_ptr() as *const StartShuttleCrankArgs) })
    }
}
