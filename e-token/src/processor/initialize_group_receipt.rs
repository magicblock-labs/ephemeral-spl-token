use crate::processor::execute_transfer_callback::{
    derive_group_receipt_id, initialize_group_receipt, log_group_receipt, read_u32_le,
    GroupReceiptController,
};
use core::num::NonZeroU32;
use ephemeral_spl_api::program::id_address;
use ephemeral_spl_api::state::group_receipt::GroupReceipt;
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, TransferQueueHeader, QUEUE_SEED,
};
use ephemeral_spl_api::Address;
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

pub fn process_initialize_group_receipt(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [validator, queue_info, group_receipt, magic_vault, magic_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let args = InitializeGroupReceiptArgs::try_from_bytes(instruction_data)?;
    let splits = NonZeroU32::new(args.splits).ok_or(ProgramError::InvalidInstructionData)?;

    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    let group_receipt_bump = validate(validator, queue_info, group_receipt, header, &args)?;

    if group_receipt.owned_by(&id_address()) {
        pinocchio_log::log!("Group receipt was initialized already!");
        handle_already_initialized_receipt(
            queue_info,
            group_receipt,
            magic_vault,
            magic_program,
            splits,
        )
    } else {
        initialize_group_receipt(
            queue_info,
            magic_vault,
            group_receipt,
            group_receipt_bump,
            args.group_id,
            splits.get(),
        )
    }
}

fn validate(
    validator: &AccountView,
    queue_info: &AccountView,
    group_receipt: &AccountView,
    header: &TransferQueueHeader,
    args: &InitializeGroupReceiptArgs,
) -> Result<u8, ProgramError> {
    if !validator.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Under condition that queue can be created only by validator
    // Verifies both validator & queue
    let (derived_queue, _) = Address::find_program_address(
        &[
            QUEUE_SEED,
            header.mint.as_ref(),
            validator.address().as_ref(),
        ],
        &id_address(),
    );

    if &derived_queue != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&id_address()) {
        return Err(ProgramError::IllegalOwner);
    }

    let (derived_group_receipt_id, bump) =
        derive_group_receipt_id(queue_info.address(), args.group_id);
    if group_receipt.address() != &derived_group_receipt_id {
        return Err(ProgramError::InvalidSeeds);
    }
    if GroupReceipt::new(group_receipt)?.id() != args.group_id {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(bump)
}

/// Handle case when some callbacks executed before crank ticked
fn handle_already_initialized_receipt(
    queue_info: &AccountView,
    group_receipt_info: &AccountView,
    magic_vault: &AccountView,
    magic_program: &AccountView,
    splits: NonZeroU32,
) -> ProgramResult {
    let mut group_receipt =
        GroupReceiptController::new(group_receipt_info, queue_info, magic_vault, magic_program)?;

    group_receipt.set_splits(splits)?;
    if splits.get() as usize <= group_receipt.items_len() {
        // All callbacks executed
        log_group_receipt(&group_receipt);
        group_receipt.close()
    } else {
        Ok(())
    }
}

pub struct InitializeGroupReceiptArgs {
    /// ID of a group receipt associated with
    group_id: u32,
    /// Number of splits for transfer
    splits: u32,
}

impl InitializeGroupReceiptArgs {
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self, ProgramError> {
        let mut cur = 0;
        let group_id = read_u32_le(bytes, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
        let splits = read_u32_le(bytes, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;

        Ok(Self { group_id, splits })
    }
}
