use pinocchio::{cpi::Seed, error::ProgramError};
use solana_address::Address;

/// Seed prefix for the swap-custody "stash" PDA.
///
/// Seeds: `[STASH_PDA_SEED, user, mint]`.
///
/// Zero-data, system-owned PDA. Used both as (a) the authority of the SPL
/// associated token account that Jupiter deposits swap output into, and
/// (b) the `payer` + `owner` signer when the scheduled private-transfer
/// flow self-CPIs into instruction 25 via `invoke_signed`.
pub const STASH_PDA_SEED: &[u8] = b"stash";

/// Stash PDA helpers.
///
/// Intentionally zero-data: the PDA exists only to own an ATA and sign
/// CPIs. Validation is by key equality only.
pub struct StashPda;

impl StashPda {
    #[inline(always)]
    pub fn find_pda(user: &Address, mint: &Address) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(user, mint), &crate::ID)
    }

    #[inline(always)]
    pub fn derive_pda(
        user: &Address,
        mint: &Address,
        bump_seed: u8,
    ) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let pda =
            Address::create_program_address(&Self::seeds_with_bump(user, mint, &bump), &crate::ID)?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn seeds<'a>(user: &'a Address, mint: &'a Address) -> [&'a [u8]; 3] {
        [STASH_PDA_SEED, user.as_ref(), mint.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(
        user: &'a Address,
        mint: &'a Address,
        bump: &'a [u8],
    ) -> [&'a [u8]; 4] {
        [STASH_PDA_SEED, user.as_ref(), mint.as_ref(), bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(user: &'a Address, mint: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 4] {
        [
            Seed::from(STASH_PDA_SEED),
            Seed::from(user.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(bump),
        ]
    }
}
