use super::{Initializable, RawType};

/// On-chain record tracking how many transfers in a group remain to be
/// confirmed.  One account is created per (queue, group_id) pair and is
/// closed once all splits have been acknowledged.
#[repr(C)]
pub struct GroupReceipt {
    /// Group ID
    pub id: u32,
    /// PDA bump for receipt.
    pub bump: u8,
    /// Explicit padding
    pub _pad: [u8; 3],
    /// How many transfers in this group are still outstanding.
    pub transfers_left: u32,
}

impl RawType for GroupReceipt {
    const LEN: usize = core::mem::size_of::<GroupReceipt>();
}

impl Initializable for GroupReceipt {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        // Initialized group ID starts from 1
        self.id != 0
    }
}

impl GroupReceipt {
    pub fn new(id: u32, bump: u8, splits: u32) -> Self {
        Self {
            id,
            bump,
            _pad: [0; 3],
            transfers_left: splits
        }
    }

    pub fn transfers_left(&self) -> u32 {
        self.transfers_left
    }

    // TODO(edwin): define API
    pub fn transfer_complete(&mut self) {
        let _ = self.transfers_left.checked_sub(1);
    }
}