use pinocchio::pubkey::Pubkey;

use super::{Initializable, RawType};

/// Internal representation of a token account data.
#[repr(C)]
pub struct EphemeralAta {
    /// The mint associated with this account
    pub mint: Pubkey,
    /// The amount of tokens this account holds.
    pub amount: u64,
}

impl RawType for EphemeralAta {
    const LEN: usize = core::mem::size_of::<EphemeralAta>();
}

impl Initializable for EphemeralAta {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Pubkey::default()
    }
}
