use pinocchio::Address;

use super::{Initializable, RawType};

pub const FEES_PDA_SEED: &[u8] = b"FEES";
pub const FEES_PDA_TAG: [u8; 4] = *b"FEES";

/// Minimal validator-scoped PDA used for fee delegation/commit flows.
#[repr(C)]
pub struct FeesPda {
    pub tag: [u8; 4],
    pub validator: Address,
    pub bump: u8,
}

impl RawType for FeesPda {
    const LEN: usize = core::mem::size_of::<FeesPda>();
}

impl Initializable for FeesPda {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.tag == FEES_PDA_TAG
    }
}
