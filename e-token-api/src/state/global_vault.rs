use pinocchio::{cpi::Seed, error::ProgramError};
use solana_address::{address_eq, Address};

use super::{Initializable, RawType};

/// Internal representation of a global vault for a specific mint.
#[repr(C)]
pub struct GlobalVault {
    /// The mint associated with this vault
    pub mint: Address,
    /// The token account that holds this vault's tokens.
    pub token_account: Address,
    /// The bump of the global vault
    pub bump: u8,
}

impl RawType for GlobalVault {
    const LEN: usize = core::mem::size_of::<GlobalVault>();
}

impl Initializable for GlobalVault {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        !address_eq(&self.mint, &Address::default())
    }
}

impl GlobalVault {
    #[inline(always)]
    pub fn derive_pda(mint: &Address, bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let pda = Address::create_program_address(&Self::seeds_with_bump(mint, &bump), &crate::ID)?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn find_pda(mint: &Address) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(mint), &crate::ID)
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
