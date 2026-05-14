use crate::processor::execute_transfer_callback::GROUP_RECEIPT_SEED;
use crate::processor::utils::{close_ephemeral_account, create_ephemeral_account};
use ephemeral_spl_api::state::group_receipt;
use ephemeral_spl_api::state::group_receipt::GroupReceipt;
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

    let group_id_bytes = group_id.to_le_bytes();
    let receipt_bump_seed = [group_receipt_bump];
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
        space
            .try_into()
            .map_err(|_| ProgramError::ArithmeticOverflow)?,
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
