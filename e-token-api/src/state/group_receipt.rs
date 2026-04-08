use bytemuck::{Pod, Zeroable};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};
use solana_signature::Signature;

pub struct GroupReceipt<'a> {
    header: &'a mut GroupReceiptHeader,
    items_data: &'a mut [u8],
    items_capacity: usize,
}

impl<'a> GroupReceipt<'a> {
    pub fn new(info: &'a AccountView) -> Result<Self, ProgramError> {
        let data = unsafe { info.borrow_unchecked_mut() };
        Ok(unsafe { Self::from_data_mut(data)? })
    }

    pub unsafe fn from_data_mut(data: &'a mut [u8]) -> Result<Self, ProgramError> {
        let (header_data, items_data) = data
            .split_at_mut_checked(GroupReceiptHeader::size())
            .ok_or(ProgramError::InvalidAccountData)?;

        // Parse header
        let header = GroupReceiptHeader::from_data_mut(header_data)?;

        // Normalize and store items data
        let items_capacity = Self::calculate_items_capacity(items_data);
        let items_data = &mut items_data[..items_capacity * TransferReceipt::size()];
        Ok(Self {
            header,
            items_data,
            items_capacity,
        })
    }

    /// Calculates required size in bytes for given number of items
    pub fn required_size(items: usize) -> usize {
        GroupReceiptHeader::size() + TransferReceipt::size() * items
    }

    pub fn items(&self) -> Result<&[TransferReceipt], ProgramError> {
        let initialized_items_bytes = self.initialized_items_bytes();
        bytemuck::try_cast_slice(&self.items_data[..initialized_items_bytes])
            .map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Records transfer, adding item and updating state accordingly
    pub fn record_transfer(&mut self, item: TransferReceipt) -> ProgramResult {
        if self.transfers_left() > 0 {
            Ok(())
        } else {
            Err(ProgramError::InvalidInstructionData)
        }?;
        let item_start = self.initialized_items_bytes();
        let item_range = item_start..item_start + TransferReceipt::size();
        self.items_data[item_range].copy_from_slice(bytemuck::bytes_of(&item));
        self.header.transfers_left -= 1;

        Ok(())
    }

    fn initialized_items_bytes(&self) -> usize {
        let initialized_items = self.items_capacity - self.transfers_left() as usize;
        initialized_items * TransferReceipt::size()
    }

    pub fn id(&self) -> u32 {
        self.header.id
    }

    pub fn transfers_left(&self) -> u32 {
        self.header.transfers_left
    }

    pub fn calculate_items_capacity(data: &[u8]) -> usize {
        data.len() / TransferReceipt::size()
    }
}

/// On-chain record tracking how many transfers in a group remain to be
/// confirmed.  One account is created per (queue, group_id) pair and is
/// closed once all splits have been acknowledged.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct GroupReceiptHeader {
    /// Group ID
    pub id: u32,
    /// How many transfers in this group are still outstanding.
    pub transfers_left: u32,
    /// PDA bump for receipt.
    pub bump: u8,
    /// Reserved for future fields without migration.
    pub _reserved: [u8; 7],
}

impl GroupReceiptHeader {
    pub fn new(id: u32, bump: u8, splits: u32) -> Self {
        Self {
            id,
            transfers_left: splits,
            bump,
            _reserved: [0; 7],
        }
    }

    pub fn from_data(data: &[u8]) -> Result<&GroupReceiptHeader, ProgramError> {
        bytemuck::try_from_bytes::<GroupReceiptHeader>(data)
            .map_err(|_| ProgramError::InvalidAccountData)
    }

    fn from_data_mut(data: &mut [u8]) -> Result<&mut GroupReceiptHeader, ProgramError> {
        bytemuck::try_from_bytes_mut::<GroupReceiptHeader>(data)
            .map_err(|_| ProgramError::InvalidAccountData)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn transfers_left(&self) -> u32 {
        self.transfers_left
    }

    pub fn bump(&self) -> u8 {
        self.bump
    }

    pub const fn size() -> usize {
        size_of::<Self>()
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TransferReceipt {
    /// Signature of transfer action, or zeros if signature was absent
    signature: Signature,
    /// Amount transferred in the action
    amount: u64,
    /// Whether the transfer action succeeded (1) or failed (0)
    ok: u8,
    _reserved: [u8; 7],
}

impl TransferReceipt {
    pub fn new(signature: Option<Signature>, amount: u64, ok: bool) -> Self {
        Self {
            signature: signature.unwrap_or(Signature::zeroed()),
            amount,
            ok: ok as u8,
            _reserved: [0; 7],
        }
    }

    pub fn signature(&self) -> Option<&Signature> {
        if self.signature == Signature::zeroed() {
            None
        } else {
            Some(&self.signature)
        }
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }

    pub fn ok(&self) -> bool {
        self.ok != 0
    }

    pub const fn size() -> usize {
        size_of::<Self>()
    }
}

pub fn initialize_group_receipt(
    account: &AccountView,
    group_id: u32,
    splits: u32,
    bump: u8,
) -> ProgramResult {
    let data = unsafe { account.borrow_unchecked_mut() };
    let required_data = GroupReceipt::required_size(splits as usize);

    if data.len() != required_data {
        Err(ProgramError::InvalidInstructionData)
    } else {
        Ok(())
    }?;

    let header = GroupReceiptHeader::new(group_id, bump, splits);
    data[..GroupReceiptHeader::size()].copy_from_slice(bytemuck::bytes_of(&header));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    fn init_data(id: u32, splits: u32, bump: u8) -> std::vec::Vec<u8> {
        let mut data = std::vec![0u8; GroupReceipt::required_size(splits as usize)];
        let header = GroupReceiptHeader::new(id, bump, splits);
        data[..GroupReceiptHeader::size()].copy_from_slice(bytemuck::bytes_of(&header));
        data
    }

    #[test]
    fn from_data_mut_parses_header() {
        let mut data = init_data(7, 3, 5);
        let gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        assert_eq!(gr.id(), 7);
        assert_eq!(gr.transfers_left(), 3);
    }

    #[test]
    fn from_data_mut_too_small_returns_error() {
        let mut data = std::vec![0u8; GroupReceiptHeader::size() - 1];
        assert!(unsafe { GroupReceipt::from_data_mut(&mut data) }.is_err());
    }

    #[test]
    fn record_transfer_decrements_transfers_left() {
        let mut data = init_data(1, 2, 0);
        let mut gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        gr.record_transfer(TransferReceipt::new(None, 100, true))
            .unwrap();
        assert_eq!(gr.transfers_left(), 1);
        gr.record_transfer(TransferReceipt::new(None, 50, false))
            .unwrap();
        assert_eq!(gr.transfers_left(), 0);
    }

    #[test]
    fn record_transfer_fails_when_exhausted() {
        let mut data = init_data(1, 1, 0);
        let mut gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        gr.record_transfer(TransferReceipt::new(None, 100, true))
            .unwrap();
        assert!(gr
            .record_transfer(TransferReceipt::new(None, 50, false))
            .is_err());
    }

    #[test]
    fn items_is_empty_before_any_transfer() {
        let mut data = init_data(1, 2, 0);
        let gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        assert_eq!(gr.items().unwrap().len(), 0);
    }

    #[test]
    fn items_reflects_recorded_transfers() {
        let mut data = init_data(1, 2, 0);
        let sig = Signature::from([3u8; 64]);
        {
            let mut gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
            gr.record_transfer(TransferReceipt::new(Some(sig), 200, true))
                .unwrap();
            gr.record_transfer(TransferReceipt::new(None, 300, false))
                .unwrap();
        }
        let gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        let items = gr.items().unwrap();
        assert_eq!(items.len(), 2);

        assert_eq!(items[0].amount(), 200);
        assert!(items[0].ok());
        assert!(items[0].signature().is_some());

        assert_eq!(items[1].amount(), 300);
        assert!(!items[1].ok());
        assert!(items[1].signature().is_none());
    }

    #[test]
    fn record_transfer_fails_with_zero_splits() {
        let mut data = init_data(1, 0, 0);
        let mut gr = unsafe { GroupReceipt::from_data_mut(&mut data) }.unwrap();
        assert!(gr
            .record_transfer(TransferReceipt::new(None, 0, false))
            .is_err());
    }
}
