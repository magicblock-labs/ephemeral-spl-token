use pinocchio::{cpi::Seed, error::ProgramError};
use solana_address::{address_eq, Address};

use super::{Initializable, RawType};

/// Internal representation of a shuttle metadata account.
#[repr(C)]
pub struct ShuttleMetadata {
    /// The logical owner of this shuttle account (seed used for shuttle PDA).
    pub owner: Address,
    /// The account that receives rent refunds for this shuttle account flow.
    pub payer: Address,
    /// User-defined identifier to allow multiple shuttles per [owner, mint].
    pub id: u32,
    /// The bump of the shuttle metadata account
    pub bump: u8,
    pub _pad0: [u8; 3],
}

impl RawType for ShuttleMetadata {
    const LEN: usize = core::mem::size_of::<ShuttleMetadata>();
}

impl Initializable for ShuttleMetadata {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        !address_eq(&self.owner, &Address::default())
    }
}

impl ShuttleMetadata {
    #[inline(always)]
    pub fn derive_pda(owner: &Address, mint: &Address, id: u32, bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let pda = Address::create_program_address(
            &Self::seeds_with_bump(owner, mint, &id.to_le_bytes(), &bump),
            &crate::ID,
        )?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn find_pda(owner: &Address, mint: &Address, id: u32) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(owner, mint, &id.to_le_bytes()), &crate::ID)
    }

    #[inline(always)]
    pub fn seeds<'a>(owner: &'a Address, mint: &'a Address, id_bytes: &'a [u8]) -> [&'a [u8]; 3] {
        [owner.as_ref(), mint.as_ref(), id_bytes]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(
        owner: &'a Address,
        mint: &'a Address,
        id_bytes: &'a [u8],
        bump: &'a [u8],
    ) -> [&'a [u8]; 4] {
        [owner.as_ref(), mint.as_ref(), id_bytes, bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(
        owner: &'a Address,
        mint: &'a Address,
        id_bytes: &'a [u8],
        bump: &'a [u8],
    ) -> [Seed<'a>; 4] {
        [
            Seed::from(owner.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(id_bytes),
            Seed::from(bump),
        ]
    }
}
