use pinocchio::{cpi::Seed, error::ProgramError, Address};

use super::{Initializable, RawType};

/// Internal representation of a shuttle ephemeral token account.
#[repr(C)]
pub struct ShuttleEphemeralAta {
    /// The canonical bump of the shuttle eata
    pub bump: u8,
    /// The logical owner of this shuttle account (seed used for shuttle PDA).
    pub owner: Address,
    /// The account that funded rent for this shuttle account.
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

impl ShuttleEphemeralAta {
    #[inline(always)]
    pub fn create_address(
        owner: &Address,
        mint: &Address,
        id: u32,
        bump: &[u8],
    ) -> Result<Address, ProgramError> {
        Address::create_program_address(
            &Self::seeds_with_bump(owner, mint, &id.to_le_bytes(), bump),
            &crate::program::id_address(),
        )
        .map_err(|_| ProgramError::InvalidSeeds)
    }

    #[inline(always)]
    pub fn find_pda(owner: &Address, mint: &Address, id: u32) -> (Address, u8) {
        Address::find_program_address(
            &[owner.as_ref(), mint.as_ref(), id.to_le_bytes().as_ref()],
            &crate::program::id_address(),
        )
    }

    #[inline(always)]
    pub fn seeds<'a>(owner: &'a Address, mint: &'a Address, id: &'a [u8; 4]) -> [&'a [u8]; 3] {
        [owner.as_ref(), mint.as_ref(), id]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(
        owner: &'a Address,
        mint: &'a Address,
        id: &'a [u8; 4],
        bump: &'a [u8],
    ) -> [&'a [u8]; 4] {
        [owner.as_ref(), mint.as_ref(), id, bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(
        owner: &'a Address,
        mint: &'a Address,
        id: &'a [u8; 4],
        bump: &'a [u8],
    ) -> [Seed<'a>; 4] {
        [
            Seed::from(owner.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(id),
            Seed::from(bump),
        ]
    }
}
