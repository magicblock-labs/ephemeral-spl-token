use pinocchio::pubkey::Pubkey;

use super::{Initializable, RawType};

/// Internal representation of a global vault for a specific mint.
#[repr(C)]
pub struct GlobalVault {
    /// The mint associated with this vault
    pub mint: Pubkey,
}

impl RawType for GlobalVault {
    const LEN: usize = core::mem::size_of::<GlobalVault>();
}

impl Initializable for GlobalVault {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Pubkey::default()
    }
}
