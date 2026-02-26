use core::marker::PhantomData;
use ephemeral_rollups_pinocchio::acl::{create_permission, MembersArgs};
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::Address;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_associated_token_account::instructions::CreateIdempotent;

use crate::processor::common::{self, create_pda, initialize_shuttle};

#[inline(always)]
pub fn process_initialize_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable, signer]         Payer (funding account)
    // 1. [writable] Transfer Queue account (PDA derived from [mint])
    // 2. []         Mint  (seed)
    // 3. [writable] Queue Permission PDA (derived from ["permission:", queue])
    // 4. [writable] Queue ATA account (ATA for [queue, mint])
    // 5. [writable] Queue EATA account (EATA for [queue, mint])
    // 6. [writable] Queue EATA Permission PDA (derived from ["permission:", queue_eata])
    // 7. [writable] Shuttle account (PDA derived from [owner, mint, shuttle_id])
    // 8. [writable] Shuttle ATA account (ATA for [shuttle, mint])
    // 9. [writable] Shuttle EATA account (EATA for [shuttle, mint])
    // 10. []         System program
    // 11. []         Token program
    // 12. []         Associated token program
    // 13. []         Permission program (ACL)

    let args = InitializeTransferQueue::try_from_bytes(instruction_data)?;

    let [
        payer_info, // force next line
        queue_info,
        mint_info,
        queue_permission_info,
        queue_ata_info,
        queue_eata_info,
        queue_eata_permission_info,
        shuttle_info,
        shuttle_ata_info,
        shuttle_eata_info,
        system_program_info,
        token_program_info,
        _associated_token_program_info,
        permission_program_info,
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let (queue_pda, queue_bump) = TransferQueue::find_pda(mint_info.address());
    if queue_info.address() != &queue_pda {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump = [queue_bump];
    let seeds = TransferQueue::signer_seeds(&mint_info.address(), &bump);
    let queue_signer = Signer::from(&seeds);

    let rent = Rent::get()?;

    create_pda(
        &rent,
        TransferQueue::LEN,
        payer_info,
        queue_info,
        &ephemeral_spl_api::program::id_address(),
        &[queue_signer.clone()],
    )?;

    // Makes the queue private
    create_permission(
        &[
            queue_info,
            queue_permission_info,
            payer_info,
            system_program_info,
        ],
        &permission_program_info.address(),
        MembersArgs { members: Some(&[]) },
        Some(queue_signer),
    )?;

    CreateIdempotent {
        funding_account: payer_info,
        account: queue_ata_info,
        wallet: queue_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    }
    .invoke()?;

    common::initialize_ephemeral_ata(
        &rent,
        payer_info,
        queue_info,
        mint_info,
        queue_eata_info,
        args.queue_eata_bump(),
    )?;

    // Create the permission for the queue EATA
    let bump = [args.queue_eata_bump()];
    let seeds = [
        Seed::from(queue_info.address().as_ref()),
        Seed::from(mint_info.address().as_ref()),
        Seed::from(&bump),
    ];
    let queue_eata_signer = Signer::from(&seeds);
    create_permission(
        &[
            queue_eata_info,
            queue_eata_permission_info,
            payer_info,
            system_program_info,
        ],
        &permission_program_info.address(),
        MembersArgs { members: Some(&[]) },
        Some(queue_eata_signer),
    )?;

    // Create the shuttle and its ATA for the queue
    let (shuttle_pda, shuttle_bump) =
        ShuttleEphemeralAta::find_pda(queue_info.address(), mint_info.address(), args.shuttle_id());
    if shuttle_info.address() != &shuttle_pda {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump = [shuttle_bump];
    initialize_shuttle(
        &rent,
        payer_info,
        queue_info,
        mint_info,
        shuttle_info,
        args.shuttle_id(),
        &bump,
    )?;

    let (shuttle_eata_pda, shuttle_eata_bump) = Address::find_program_address(
        &[
            shuttle_info.address().as_ref(),
            mint_info.address().as_ref(),
        ],
        &ephemeral_spl_api::program::id_address(),
    );
    if &shuttle_eata_pda != shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    common::initialize_ephemeral_ata(
        &rent,
        payer_info,
        shuttle_info,
        mint_info,
        shuttle_eata_info,
        shuttle_eata_bump,
    )?;
    CreateIdempotent {
        funding_account: payer_info,
        account: shuttle_ata_info,
        wallet: shuttle_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    }
    .invoke()?;

    // Initialize the queue
    let queue = unsafe { load_mut_unchecked::<TransferQueue>(queue_info.borrow_unchecked_mut())? };
    queue.mint = mint_info.address().clone();
    queue.bump = queue_bump;

    Ok(())
}

/// Instruction data for the `InitializeMint` instruction.
pub struct InitializeTransferQueue<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeTransferQueue<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeTransferQueue, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeTransferQueue {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn queue_eata_bump(&self) -> u8 {
        unsafe { *self.raw }
    }

    #[inline]
    pub fn shuttle_id(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(1), buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }
}
