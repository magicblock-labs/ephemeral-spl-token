use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use ephemeral_spl_api::state::RawType;
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::common::create_pda;

const MAX_ALLOCATION_SPACE: usize = 10240;

#[inline(always)]
pub fn process_allocate_queue(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    // Expected accounts:
    // 0. [writable, signer]    Payer (funding account)
    // 1. [writable]            Transfer Queue account (PDA derived from [mint])
    // 2. []                    Mint
    // 3. []                    System program

    let [
        payer_info, // force next line
        queue_info,
        mint_info,
        _system_program_info,
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let remaining_space = if !queue_info.owned_by(&ephemeral_spl_api::program::id_address()) {
        // First call, create the account
        let (queue_pda, queue_bump) = TransferQueue::find_pda(mint_info.address());
        if queue_info.address() != &queue_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        let bump = [queue_bump];
        let seeds = TransferQueue::signer_seeds(&mint_info.address(), &bump);
        let queue_signer = Signer::from(&seeds);

        create_pda(
            &Rent::get()?,
            41, // Allocate just the first few bytes
            TransferQueue::LEN,
            payer_info,
            queue_info,
            &ephemeral_spl_api::program::id_address(),
            &[queue_signer.clone()],
        )?;

        // SAFETY: We know the queue account is freshly created and has space for the bump.
        unsafe {
            queue_info.borrow_unchecked_mut()[0] = queue_bump;
        };

        MAX_ALLOCATION_SPACE - 41
    } else {
        let is_initialized = unsafe {
            let data = queue_info.borrow_unchecked();
            data[1..33] != [0; 32]
        };
        if is_initialized {
            return Err(ProgramError::AccountAlreadyInitialized);
        }
        if queue_info.data_len() >= TransferQueue::LEN {
            // Already allocated and initialized the queue
            return Ok(());
        }

        (TransferQueue::LEN - queue_info.data_len()).min(MAX_ALLOCATION_SPACE)
    };

    queue_info.resize(queue_info.data_len() + remaining_space)?;

    Ok(())
}
