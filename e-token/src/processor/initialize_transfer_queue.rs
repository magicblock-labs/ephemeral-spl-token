use core::marker::PhantomData;
use ephemeral_spl_api::state::transfer_queue::{
    capacity_from_data_len, header_len, init_queue, item_len, queue_views_mut_checked, QUEUE_SEED,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_system::instructions::CreateAccount;

pub const DEFAULT_TRANSFER_QUEUE_ITEMS: u32 = 92;
/// Default queue size in bytes. (HEADER_LEN + ITEM_LEN * DEFAULT_TRANSFER_QUEUE_ITEMS)
pub const DEFAULT_TRANSFER_QUEUE_SIZE_BYTES: u64 = 96 + 104 * DEFAULT_TRANSFER_QUEUE_ITEMS as u64;

#[inline(always)]
pub fn process_initialize_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer (funds account creation)
    // 1. [writable] Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator])
    // 2. []         Mint account (seed)
    // 3. []         Validator
    // 4. []         System program
    let args = InitializeTransferQueueArgs::try_from_bytes(instruction_data)?;

    let [payer_info, queue_info, mint_info, validator_info, _system_program_info, ..] = accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let (requested_items, queue_size) = if let Some(items) = args.requested_items() {
        (items, header_len() + item_len() * items as usize)
    } else {
        (
            DEFAULT_TRANSFER_QUEUE_ITEMS,
            DEFAULT_TRANSFER_QUEUE_SIZE_BYTES as usize,
        )
    };
    if requested_items == 0 {
        return Err(ProgramError::InvalidInstructionData);
    };

    let program_id = ephemeral_spl_api::program::id_address();
    let (derived_queue, bump) = ephemeral_spl_api::Address::find_program_address(
        &[
            QUEUE_SEED,
            mint_info.address().as_ref(),
            validator_info.address().as_ref(),
        ],
        &program_id,
    );
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
        let signer_seeds = [
            Seed::from(QUEUE_SEED),
            Seed::from(mint_info.address().as_ref()),
            Seed::from(validator_info.address().as_ref()),
            Seed::from(&bump_seed),
        ];
        let signer = Signer::from(&signer_seeds);

        // Allocate the default space but fund the desired size's rent.
        CreateAccount {
            from: payer_info,
            to: queue_info,
            space: (queue_size as u64).min(DEFAULT_TRANSFER_QUEUE_SIZE_BYTES),
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
    init_queue(data, bump, *mint_info.address(), *validator_info.address())?;

    let (header, _) = queue_views_mut_checked(data)?;
    if header.bump != bump
        || header.mint != *mint_info.address()
        || header.validator != *validator_info.address()
    {
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
    pub fn requested_items(&self) -> Option<u32> {
        if self.len == 0 {
            None
        } else {
            let mut buf = [0u8; 4];
            unsafe {
                core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
            }
            Some(u32::from_le_bytes(buf))
        }
    }
}
