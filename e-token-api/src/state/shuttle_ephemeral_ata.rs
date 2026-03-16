use pinocchio::Address;

use super::{Initializable, RawType};

/// Internal representation of a shuttle ephemeral token account.
#[repr(C)]
pub struct ShuttleEphemeralAta {
    /// The logical owner of this shuttle account (seed used for shuttle PDA).
    pub owner: Address,
    /// The account that receives rent refunds for this shuttle account flow.
    pub payer: Address,
    /// User-defined identifier to allow multiple shuttles per [owner, mint].
    pub id: u32,
}

impl RawType for ShuttleEphemeralAta {
    const LEN: usize = core::mem::size_of::<ShuttleEphemeralAta>();
}

impl Initializable for ShuttleEphemeralAta {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.owner != Address::default()
    }
}
