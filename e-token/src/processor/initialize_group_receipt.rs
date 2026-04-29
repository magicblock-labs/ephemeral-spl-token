use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;

use crate::processor::execute_transfer_callback::derive_group_receipt_id;
use crate::processor::utils::{GroupReceiptController, CRANK_SIGNER};
use core::num::NonZeroU32;
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, TransferQueueHeader, QUEUE_SEED,
};
use ephemeral_spl_api::Address;
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: [signer]            - PDA     : Crank authenticator.
///  1: [writable]          - PDA     : Queue.
///  2: [writable]          - PDA     : Group Receipt, ephemeral account.
///  3: [writable]          - PDA     : Magic vault.
///  4: []                  - Magic   : Magic program.
///
/// Instruction Data: InitializeGroupReceiptArgs
///
pub fn process_initialize_group_receipt(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [crank_signer, queue_info, group_receipt, magic_vault, magic_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let args = InitializeGroupReceiptArgs::decode(instruction_data)?;
    let splits = NonZeroU32::new(args.splits()).ok_or(ProgramError::InvalidInstructionData)?;

    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    let group_receipt_bump = validate(crank_signer, queue_info, group_receipt, header, &args)?;

    if group_receipt.owned_by(&crate::ID) {
        #[cfg(feature = "logging")]
        pinocchio_log::log!("Group receipt was initialized already!");

        handle_already_initialized_receipt(
            queue_info,
            group_receipt,
            magic_vault,
            magic_program,
            args.group_id(),
            splits,
        )
    } else {
        GroupReceiptController::create(
            group_receipt,
            queue_info,
            magic_vault,
            magic_program,
            group_receipt_bump,
            args.group_id(),
            splits.get(),
        )?;

        Ok(())
    }
}

fn validate(
    crank_signer: &AccountView,
    queue_info: &AccountView,
    group_receipt: &AccountView,
    header: &TransferQueueHeader,
    args: &InitializeGroupReceiptArgsView<'_>,
) -> Result<u8, ProgramError> {
    if !crank_signer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if crank_signer.address() != &CRANK_SIGNER {
        return Err(ProgramError::IncorrectAuthority);
    }

    // Under condition that queue can be created only by validator
    // Verifies both validator & queue
    let (derived_queue, _) = Address::find_program_address(
        &[QUEUE_SEED, header.mint.as_ref(), header.validator.as_ref()],
        &crate::ID,
    );

    if &derived_queue != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&crate::ID) {
        return Err(ProgramError::IllegalOwner);
    }

    let (derived_group_receipt_id, bump) =
        derive_group_receipt_id(queue_info.address(), args.group_id());
    if group_receipt.address() != &derived_group_receipt_id {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(bump)
}

/// Handle case when some callbacks executed before crank ticked
fn handle_already_initialized_receipt(
    queue_info: &AccountView,
    group_receipt_info: &AccountView,
    magic_vault: &AccountView,
    magic_program: &AccountView,
    group_id: u32,
    splits: NonZeroU32,
) -> ProgramResult {
    let mut group_receipt =
        GroupReceiptController::view(group_receipt_info, queue_info, magic_vault, magic_program)?;

    if group_receipt.id() != group_id {
        return Err(ProgramError::InvalidInstructionData);
    }

    group_receipt.set_splits(splits)?;
    if splits.get() as usize <= group_receipt.items_len() {
        // All callbacks executed
        #[cfg(feature = "logging")]
        group_receipt.log();

        group_receipt.close()
    } else {
        Ok(())
    }
}

#[variable_offset_layout(buffer_offset = 1)]
pub struct InitializeGroupReceiptArgs {
    /// ID of a group receipt associated with
    pub group_id: u32,
    /// Number of splits for transfer
    pub splits: u32,
}
