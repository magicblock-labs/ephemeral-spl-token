use pinocchio::{cpi::Seed, error::ProgramError, Address};

use super::{Initializable, RawType};

/// Internal representation of a token account data.
#[repr(C)]
pub struct EphemeralAta {
    /// The canonical bump of the eata
    pub bump: u8,
    /// The owner of the eata
    pub owner: Address,
    /// The mint associated with this account
    pub mint: Address,
    /// The amount of tokens this account holds.
    pub amount: u64,
}

impl RawType for EphemeralAta {
    const LEN: usize = core::mem::size_of::<EphemeralAta>();
}

impl Initializable for EphemeralAta {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Address::default()
    }
}

impl EphemeralAta {
    #[inline(always)]
    pub fn create_address(
        owner: &Address,
        mint: &Address,
        bump: &[u8],
    ) -> Result<Address, ProgramError> {
        Address::create_program_address(
            &[owner.as_ref(), mint.as_ref(), bump.as_ref()],
            &crate::program::id_address(),
        )
        .map_err(|_| ProgramError::InvalidSeeds)
    }

    #[inline(always)]
    pub fn find_pda(owner: &Address, mint: &Address) -> (Address, u8) {
        Address::find_program_address(
            &[owner.as_ref(), mint.as_ref()],
            &crate::program::id_address(),
        )
    }

    #[inline(always)]
    pub fn seeds<'a>(owner: &'a Address, mint: &'a Address) -> [&'a [u8]; 2] {
        [owner.as_ref(), mint.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(
        owner: &'a Address,
        mint: &'a Address,
        bump: &'a [u8],
    ) -> [&'a [u8]; 3] {
        [owner.as_ref(), mint.as_ref(), bump.as_ref()]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(
        owner: &'a Address,
        mint: &'a Address,
        bump: &'a [u8],
    ) -> [Seed<'a>; 3] {
        [
            Seed::from(owner.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(bump.as_ref()),
        ]
    }
}
