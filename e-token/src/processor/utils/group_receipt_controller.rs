use crate::processor::execute_transfer_callback::{derive_group_receipt_id, GROUP_RECEIPT_SEED};
use crate::processor::utils::ephemeral_account::{
    close_ephemeral_account, create_ephemeral_account, resize_ephemeral_account,
};
use core::num::NonZeroU32;
use ephemeral_spl_api::state::group_receipt;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, TransferReceipt};
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, QUEUE_SEED};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

/// Required accounts for control over receipt
pub struct GroupReceiptAccounts<'a> {
    pub group_receipt_info: &'a AccountView,
    pub queue_info: &'a AccountView,
    pub source: &'a AccountView,
    pub magic_vault: &'a AccountView,
    pub _magic_program: &'a AccountView,
}

impl<'a> GroupReceiptAccounts<'a> {
    pub fn new(
        group_receipt_info: &'a AccountView,
        queue_info: &'a AccountView,
        source: &'a AccountView,
        magic_vault: &'a AccountView,
        magic_program: &'a AccountView,
    ) -> Self {
        Self {
            group_receipt_info,
            queue_info,
            source,
            magic_vault,
            _magic_program: magic_program,
        }
    }
}

/// Creates `GroupReceipt` and initializes it.
/// Use this when the receipt account does not yet exist.
pub fn group_receipt_create<'a>(
    accounts: &GroupReceiptAccounts<'a>,
    group_receipt_bump: u8,
    group_id: u32,
    splits: u32,
) -> Result<GroupReceipt<'a>, ProgramError> {
    let (header, _) = queue_views_checked(unsafe { accounts.queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let queue_signer = Signer::from(&queue_signer_seeds);

    let (_, receipt_bump) = derive_group_receipt_id(
        accounts.queue_info.address(),
        accounts.source.address(),
        group_id,
    );

    let group_id_bytes = group_id.to_le_bytes();
    let receipt_bump_seed = [receipt_bump];
    let receipt_signer_seeds = [
        Seed::from(GROUP_RECEIPT_SEED),
        Seed::from(accounts.queue_info.address().as_ref()),
        Seed::from(accounts.source.address().as_ref()),
        Seed::from(group_id_bytes.as_ref()),
        Seed::from(&receipt_bump_seed),
    ];
    let receipt_signer = Signer::from(&receipt_signer_seeds);

    let space = GroupReceipt::required_size(splits as usize);
    create_ephemeral_account(
        accounts.queue_info,
        accounts.group_receipt_info,
        accounts.magic_vault,
        space as u32,
        &[queue_signer, receipt_signer],
    )?;
    group_receipt::initialize_group_receipt(
        accounts.group_receipt_info,
        group_id,
        splits,
        group_receipt_bump,
    )?;

    GroupReceipt::new(accounts.group_receipt_info)
}

/// Fully initialized `GroupReceipt` with number of splits
pub fn group_receipt_set_splits<'a>(
    accounts: &GroupReceiptAccounts<'a>,
    group_receipt: &mut GroupReceipt<'a>,
    splits: NonZeroU32,
) -> ProgramResult {
    group_receipt.set_splits(splits);

    let current_capacity = group_receipt.items_capacity();
    let final_capacity = splits.get() as usize;
    if current_capacity < final_capacity {
        group_receipt_allocate_items(accounts, group_receipt, final_capacity - current_capacity)
    } else {
        Ok(())
    }
}

/// Closes the group receipt account, refunding rent to the queue PDA.
/// Consumes the receipt since the account is no longer valid after closing.
pub fn group_receipt_close(
    accounts: &GroupReceiptAccounts<'_>,
    _group_receipt: GroupReceipt<'_>,
) -> ProgramResult {
    let (header, _) = queue_views_checked(unsafe { accounts.queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let queue_signer = Signer::from(&queue_signer_seeds);
    close_ephemeral_account(
        accounts.queue_info,
        accounts.group_receipt_info,
        accounts.magic_vault,
        &[queue_signer],
    )
}

#[cfg(feature = "logging")]
#[inline(never)]
pub(crate) fn group_receipt_log(group_receipt: &GroupReceipt<'_>) {
    use alloc::string::ToString;

    pinocchio_log::log!(
        "All transfers complete for group id: {} splits: {}",
        group_receipt.id(),
        group_receipt.splits()
    );
    if let Ok(items) = group_receipt.items() {
        for (i, item) in items.iter().enumerate() {
            match item.signature() {
                Some(sig) => pinocchio_log::log!(
                    "transfer[{}], ok: {}, amount: {}, sig: {}",
                    i as u32,
                    item.ok(),
                    item.amount(),
                    sig.to_string().as_str()
                ),
                None => pinocchio_log::log!(
                    "transfer[{}], ok: {}, amount: {}, sig: None",
                    i as u32,
                    item.ok(),
                    item.amount()
                ),
            }
        }
    }
}

pub fn group_receipt_record_transfer<'a>(
    accounts: &GroupReceiptAccounts<'a>,
    group_receipt: &mut GroupReceipt<'a>,
    item: TransferReceipt,
) -> ProgramResult {
    let Err(item) = group_receipt.record_transfer(item) else {
        return Ok(());
    };

    if group_receipt.is_fully_initialized() {
        // More results than splits :)
        Err(ProgramError::InvalidInstructionData)
    } else {
        Ok(())
    }?;

    // Account not fully initialized, but we got callback result - record
    group_receipt_allocate_items(accounts, group_receipt, 1)?;
    // Now there's capacity to record the transfer
    group_receipt
        .record_transfer(item)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(())
}

/// Resizes account to hold `num` extra accounts
pub fn group_receipt_allocate_items<'a>(
    accounts: &GroupReceiptAccounts<'a>,
    group_receipt: &mut GroupReceipt<'a>,
    num: usize,
) -> ProgramResult {
    let current_len = group_receipt.items_len();

    let new_len = GroupReceipt::required_size(
        current_len
            .checked_add(num)
            .ok_or(ProgramError::ArithmeticOverflow)?,
    );

    // Create signer for resize
    let data = unsafe { accounts.queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    let queue_bump_seed = [header.bump];
    let seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let signer = Signer::from(&seeds);

    resize_ephemeral_account(
        accounts.queue_info,
        accounts.group_receipt_info,
        accounts.magic_vault,
        new_len
            .try_into()
            .map_err(|_| ProgramError::ArithmeticOverflow)?,
        &[signer],
    )?;

    *group_receipt = GroupReceipt::new(accounts.group_receipt_info)?;
    Ok(())
}
