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

        // Narmalize and store items data
        let items_capacity = Self::calculate_items_capacity(items_data);
        let items_data = &mut items_data[..items_capacity];
        Ok(Self {
            header,
            items_data,
            items_capacity,
        })
    }

    /// Calculates required size in bytes for given number of items
    pub fn required_size(items: usize) -> usize {
        GroupReceiptHeader::size() + Item::size() * items
    }

    pub fn items(&self) -> Result<&[Item], ProgramError> {
        let initialized_items_bytes = self.initialized_items_bytes();
        bytemuck::try_cast_slice(&self.items_data[..initialized_items_bytes])
            .map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Records transfer, adding item and updating state accordingly
    pub fn record_transfer(&mut self, signature: Option<Signature>) -> ProgramResult {
        if self.transfers_left() > 0 {
            Ok(())
        } else {
            Err(ProgramError::InvalidInstructionData)
        }?;

        let item = Item::new(signature);
        let item_start = self.initialized_items_bytes();
        let item_range = item_start..item_start + Item::size();
        self.items_data[item_range].copy_from_slice(bytemuck::bytes_of(&item));
        self.header.transfers_left -= 1;

        Ok(())
    }

    fn initialized_items_bytes(&self) -> usize {
        let initialized_items = self.items_capacity - self.transfers_left() as usize;
        initialized_items * Item::size()
    }

    pub fn transfers_left(&self) -> u32 {
        self.header.transfers_left
    }

    pub fn calculate_items_capacity(data: &[u8]) -> usize {
        data.len() / Item::size()
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
pub struct Item {
    /// Signature of transfer action, or zeros if signature was absent
    signature: Signature,
    _reserved: [u8; 8],
}

impl Item {
    pub fn new(signature: Option<Signature>) -> Self {
        Self {
            signature: signature.unwrap_or(Signature::zeroed()),
            _reserved: [0; 8],
        }
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
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
