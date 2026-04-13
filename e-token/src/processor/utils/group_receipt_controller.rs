use crate::processor::execute_transfer_callback::GROUP_RECEIPT_SEED;
use crate::processor::utils::ephemeral_account::{
    close_ephemeral_account, create_ephemeral_account, resize_ephemeral_account,
};
use core::num::NonZeroU32;
use core::ops::Deref;
use ephemeral_spl_api::state::group_receipt;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, TransferReceipt};
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, QUEUE_SEED};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

pub struct GroupReceiptController<'a> {
    // Required accounts for control over receipt
    group_receipt_info: &'a AccountView,
    queue_info: &'a AccountView,
    magic_vault: &'a AccountView,
    _magic_program: &'a AccountView,

    // Current view on receipt
    group_receipt: GroupReceipt<'a>,
}

impl<'a> GroupReceiptController<'a> {
    pub fn view(
        group_receipt_info: &'a AccountView,
        queue_info: &'a AccountView,
        magic_vault: &'a AccountView,
        _magic_program: &'a AccountView,
    ) -> Result<Self, ProgramError> {
        let group_receipt = GroupReceipt::new(group_receipt_info)?;
        Ok(Self {
            group_receipt_info,
            queue_info,
            magic_vault,
            _magic_program,
            group_receipt,
        })
    }

    /// Creates a new ephemeral account for the group receipt and initializes it.
    /// Use this when the receipt account does not yet exist.
    pub fn create(
        group_receipt_info: &'a AccountView,
        queue_info: &'a AccountView,
        magic_vault: &'a AccountView,
        magic_program: &'a AccountView,
        group_receipt_bump: u8,
        group_id: u32,
        splits: u32,
    ) -> Result<Self, ProgramError> {
        let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
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
            Seed::from(queue_info.address().as_ref()),
            Seed::from(group_id_bytes.as_ref()),
            Seed::from(&receipt_bump_seed),
        ];
        let receipt_signer = Signer::from(&receipt_signer_seeds);

        let space = GroupReceipt::required_size(splits as usize);
        create_ephemeral_account(
            queue_info,
            group_receipt_info,
            magic_vault,
            space as u32,
            &[queue_signer, receipt_signer],
        )?;
        group_receipt::initialize_group_receipt(
            group_receipt_info,
            group_id,
            splits,
            group_receipt_bump,
        )?;

        Self::view(group_receipt_info, queue_info, magic_vault, magic_program)
    }

    /// Returns `true` if splits are set and not 0
    pub fn is_fully_initialized(&self) -> bool {
        self.group_receipt.is_fully_initialized()
    }

    /// Returns `true` if all transfers are completed
    pub fn all_transfer_completed(&self) -> bool {
        self.group_receipt.all_transfer_completed()
    }

    /// Fully initialized `GroupReceipt` with number of splits
    pub fn set_splits(&mut self, splits: NonZeroU32) -> ProgramResult {
        self.group_receipt.set_splits(splits);

        let current_capacity = self.group_receipt.items_capacity();
        let final_capacity = splits.get() as usize;
        if current_capacity < final_capacity {
            self.allocate_items(final_capacity - current_capacity)
        } else {
            Ok(())
        }
    }

    pub fn record_transfer(&mut self, item: TransferReceipt) -> ProgramResult {
        let Err(item) = self.group_receipt.record_transfer(item) else {
            return Ok(());
        };

        if self.is_fully_initialized() {
            // More results than splits :)
            Err(ProgramError::InvalidInstructionData)
        } else {
            Ok(())
        }?;

        // Account not fully initialized, but we got callback result - record
        self.allocate_items(1)?;
        // Now there's capacity to record the transfer
        self.group_receipt
            .record_transfer(item)
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(())
    }

    /// Closes the group receipt account, refunding rent to the queue PDA.
    /// Consumes the controller since the account is no longer valid after closing.
    pub fn close(self) -> ProgramResult {
        let (header, _) = queue_views_checked(unsafe { self.queue_info.borrow_unchecked() })?;
        let queue_bump_seed = [header.bump];
        let queue_signer_seeds = [
            Seed::from(QUEUE_SEED),
            Seed::from(header.mint.as_ref()),
            Seed::from(header.validator.as_ref()),
            Seed::from(&queue_bump_seed),
        ];
        let queue_signer = Signer::from(&queue_signer_seeds);
        close_ephemeral_account(
            self.queue_info,
            self.group_receipt_info,
            self.magic_vault,
            &[queue_signer],
        )
    }

    /// Resizes account to hold `num` extra accounts
    fn allocate_items(&mut self, num: usize) -> ProgramResult {
        let current_len = self.group_receipt.items_len();
        let new_len = GroupReceipt::required_size(
            current_len
                .checked_add(num)
                .ok_or(ProgramError::ArithmeticOverflow)?,
        );

        // Create signer for resize
        let data = unsafe { self.queue_info.borrow_unchecked() };
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
            self.queue_info,
            self.group_receipt_info,
            self.magic_vault,
            new_len
                .try_into()
                .map_err(|_| ProgramError::ArithmeticOverflow)?,
            &[signer],
        )?;

        self.group_receipt = GroupReceipt::new(self.group_receipt_info)?;
        Ok(())
    }
}

impl<'a> Deref for GroupReceiptController<'a> {
    type Target = GroupReceipt<'a>;
    fn deref(&self) -> &Self::Target {
        &self.group_receipt
    }
}
