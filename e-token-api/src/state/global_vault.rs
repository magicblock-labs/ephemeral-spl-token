use pinocchio::{cpi::Seed, error::ProgramError, Address};

use super::{Initializable, RawType};

/// Internal representation of a global vault for a specific mint.
#[repr(C)]
pub struct GlobalVault {
    /// The canonical bump of the global vault
    pub bump: u8,
    /// The mint associated with this vault
    pub mint: Address,
    /// The token account that holds this vault's tokens.
    pub token_account: Address,
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

impl GlobalVault {
    #[inline(always)]
    pub fn create_address(mint: &Address, bump: &[u8]) -> Result<Address, ProgramError> {
        Address::create_program_address(&[mint.as_ref(), bump], &crate::program::id_address())
            .map_err(|_| ProgramError::InvalidSeeds)
    }

    #[inline(always)]
    pub fn find_pda(mint: &Address) -> (Address, u8) {
        Address::find_program_address(&[mint.as_ref()], &crate::program::id_address())
    }

    #[inline(always)]
    pub fn seeds(mint: &Address) -> [&[u8]; 1] {
        [mint.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(mint: &'a Address, bump: &'a [u8]) -> [&'a [u8]; 2] {
        [mint.as_ref(), bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(mint: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 2] {
        [Seed::from(mint.as_ref()), Seed::from(bump)]
    }
}
