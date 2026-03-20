use pinocchio::Address;

use super::{Initializable, RawType};

/// Internal representation of a shuttle metadata account.
#[repr(C)]
pub struct ShuttleMetadata {
    /// The logical owner of this shuttle account (seed used for shuttle PDA).
    pub owner: Address,
    /// The account that receives rent refunds for this shuttle account flow.
    pub payer: Address,
    /// User-defined identifier to allow multiple shuttles per [owner, mint].
    pub id: u32,
    /// The bump of the shuttle metadata account
    pub bump: u8,
}

impl RawType for ShuttleMetadata {
    const LEN: usize = core::mem::size_of::<ShuttleMetadata>();
}

impl Initializable for ShuttleMetadata {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.owner != Address::default()
    }
}
