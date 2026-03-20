use pinocchio::Address;

use super::{Initializable, RawType};

/// Four-byte account discriminator, reused as the PDA seed prefix for
/// validator-scoped fee delegation accounts.
pub const FEES_PDA_TAG: [u8; 4] = *b"FEES";
pub const FEES_PDA_SEED: &[u8] = &FEES_PDA_TAG;

/// Validator-scoped PDA that anchors fee delegation and commit flows.
///
/// The account does not track fee amounts. It only stores the validator and bump
/// for the derived `["FEES", validator]` address so the program can validate the
/// account and sign for delegation/commit operations.
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
