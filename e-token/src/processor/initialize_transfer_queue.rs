use core::marker::PhantomData;
use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, header_len, init_queue, item_len, queue_views_mut_checked,
    TransferQueue,
};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

pub const DEFAULT_TRANSFER_QUEUE_SIZE_BYTES: usize = 9_728;

#[inline(always)]
pub fn process_initialize_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer (funds account creation)
    // 1. [writable] Transfer queue account (PDA derived from [QUEUE_SEED, mint])
    // 2. []         Mint account (seed)
    // 3. []         System program
    let args = InitializeTransferQueueArgs::try_from_bytes(instruction_data)?;

    let [payer_info, queue_info, mint_info, _system_program_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let requested_size = args.size_bytes() as usize;
    let queue_size = if requested_size == 0 {
        DEFAULT_TRANSFER_QUEUE_SIZE_BYTES
    } else {
        requested_size
    };

    let min_size = header_len() + item_len();
    if queue_size < min_size {
        return Err(ProgramError::InvalidInstructionData);
    }

    let program_id = crate::ID;
    let (derived_queue, bump) = TransferQueue::find_pda(mint_info.address());
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if !queue_info.owned_by(&program_id) {
        if queue_info.lamports() > 0 {
            return Err(ProgramError::IllegalOwner);
        }

        if !payer_info.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let bump_seed = [bump];
        let signer_seeds = TransferQueue::signer_seeds(mint_info.address(), &bump_seed);
        let signer = Signer::from(&signer_seeds);

        CreateAccount {
            from: payer_info,
            to: queue_info,
            space: queue_size as u64,
            lamports: Rent::get()?.try_minimum_balance(queue_size)?,
            owner: &program_id,
        }
        .invoke_signed(&[signer])?;
    }

    let data_len = queue_info.data_len();
    if data_len < header_len() || capacity_from_data_len(data_len) == 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    init_queue(data, bump, *mint_info.address())?;

    let (header, _) = queue_views_mut_checked(data)?;
    if header.bump != bump || header.mint != *mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

pub struct InitializeTransferQueueArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeTransferQueueArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeTransferQueueArgs<'_>, ProgramError> {
        if !bytes.is_empty() && bytes.len() != 4 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeTransferQueueArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn size_bytes(&self) -> u32 {
        if self.len == 0 {
            0
        } else {
            let mut buf = [0u8; 4];
            unsafe {
                core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
            }
            u32::from_le_bytes(buf)
        }
    }
}
