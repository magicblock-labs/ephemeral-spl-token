use pinocchio::Address;

use super::{Initializable, RawType};

/// Internal representation of a global vault for a specific mint.
#[repr(C)]
pub struct GlobalVault {
    /// The mint associated with this vault
    pub mint: Address,
}

impl RawType for GlobalVault {
    const LEN: usize = core::mem::size_of::<GlobalVault>();
}

impl Initializable for GlobalVault {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Address::default()
    }
}
