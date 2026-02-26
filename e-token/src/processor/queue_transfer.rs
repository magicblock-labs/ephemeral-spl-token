use core::marker::PhantomData;
use ephemeral_spl_api::{
    constants::{MAX_TRANSFER_DURATION_SECONDS, MIN_CHUNK_SIZE_BPS},
    error::EphemeralSplError,
    state::transfer_queue::{QueuedTransfer, TransferQueue},
};
use pinocchio_token_2022::instructions::TransferChecked;

use {
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
    pinocchio_token_2022::state::Mint,
};

#[inline(always)]
pub fn process_queue_transfer(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let args = QueueTransferArgs::try_from_bytes(instruction_data)?;

    let [
        mint_info, // readonly
        depositor_info, // signer
        depositor_ata_info, // writable
        queue_info, // writable, PDA [mint]
        queue_ata_info, // writable, ATA for [depositor, mint]
        token_program_info, // []
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Validate Queue ownership first, before reading raw data.
    unsafe {
        if queue_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let queue = unsafe { load_mut_unchecked::<TransferQueue>(queue_info.borrow_unchecked_mut())? };

    let queue_pda = TransferQueue::create_address(&mint_info.address(), &[queue.bump])?;
    if &queue_pda != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if queue.length as usize >= queue.queue.len() {
        return Err(EphemeralSplError::QueueFull.into());
    }

    if args.amount() == &0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let chunks = args.amount() / args.chunk_size();
    if chunks < MIN_CHUNK_SIZE_BPS as u64 {
        return Err(ProgramError::InvalidInstructionData);
    }

    if chunks * (*args.interval_seconds() as u64) > MAX_TRANSFER_DURATION_SECONDS as u64 {
        return Err(ProgramError::InvalidInstructionData);
    }

    queue.queue[queue.length as usize] = QueuedTransfer {
        source: depositor_ata_info.address().clone(),
        destination: depositor_info.address().clone(),
        amount: *args.amount(),
        chunk_size: *args.chunk_size(),
        interval_seconds: *args.interval_seconds(),
    };
    queue.length += 1;

    let decimals = {
        let mint_data = unsafe { mint_info.borrow_unchecked() };
        if mint_data.len() < Mint::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
        mint.decimals()
    };

    TransferChecked {
        mint: mint_info,
        from: depositor_ata_info,
        to: queue_ata_info,
        authority: depositor_info,
        amount: *args.amount(),
        decimals,
        token_program: token_program_info.address(),
    }
    .invoke()?;

    Ok(())
}

/// Instruction data for the `QueueTransfer` instruction.
pub struct QueueTransferArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl QueueTransferArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<QueueTransferArgs, ProgramError> {
        if bytes.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(QueueTransferArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> &u64 {
        // read LE u64 from bytes[0..8]
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 8);
        }
        unsafe { &*(self.raw as *const u64) }
    }

    #[inline]
    pub fn chunk_size(&self) -> &u64 {
        // read LE u64 from bytes[8..16]
        unsafe { &*(self.raw.add(8) as *const u64) }
    }

    #[inline]
    pub fn interval_seconds(&self) -> &u16 {
        // read LE u16 from bytes[16..18]
        unsafe { &*(self.raw.add(16) as *const u16) }
    }
}
