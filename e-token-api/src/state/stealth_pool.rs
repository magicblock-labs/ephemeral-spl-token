use bytemuck::{Pod, Zeroable};
use pinocchio::{cpi::Seed, error::ProgramError};
use solana_address::Address;

use crate::{
    require, require_eq_keys,
    state::{Initializable, RawType},
};

// TODO (snawaz): can be replaced with fixed_offset_layout, or
// variable_offset_layout that provides flexibility.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct StealthPool {
    // Type marker that prevents unrelated program-owned accounts from resolving as pools.
    pub discriminator: [u8; 8],
    pub bump: u8,
    //
    // Pool-level behavior config. When FLAG_SPLIT_ACROSS_KEYS is set,
    // split payments may resolve each split independently across destination
    // keys; otherwise all splits in a payment group resolve to the same key.
    //
    // Unknown flag bits are rejected during pool initialization.
    //
    pub flags: u8,
    pub authority: Address,
    //
    // Deterministic handle identifier, usually `hash(canonical_handle)`
    // where canonical_handle could be human-readable id such as `magicblock.id`
    // or `myname@mydomain`.
    pub handle_hash: [u8; 32],
    //
    // Exact UTF-8 bytes used to derive `handle_hash`, stored for off-chain
    // display and reverse lookup. The program treats this as opaque bytes other
    // than the length cap and hash match enforced during updates.
    //
    pub handle_len: u8,
    pub handle: [u8; 255],
    pub destination_count: u8,
    pub destinations: [Address; 10],
}

impl Initializable for StealthPool {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.discriminator == StealthPool::DISCRIMINATOR
            && self.handle_len != 0
            && self.destination_count != 0
            && self.destination_count as usize <= StealthPool::MAX_DESTINATIONS
    }
}

impl RawType for StealthPool {
    const LEN: usize = core::mem::size_of::<StealthPool>();
}

impl StealthPool {
    // The discriminator has name + version
    pub const DISCRIMINATOR: [u8; 8] = *b"stpool@2";

    pub const SEED: &'static [u8] = b"stealth_pool";

    pub const MAX_HANDLE_BYTES: usize = 255;

    pub const MAX_DESTINATIONS: usize = 10;

    #[inline(always)]
    pub fn derive_pda(handle_hash: &[u8; 32], bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        Ok(Address::create_program_address(
            &Self::seeds_with_bump(handle_hash, &bump),
            &crate::ID,
        )?)
    }

    #[inline(always)]
    pub fn find_pda(handle_hash: &[u8; 32]) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(handle_hash), &crate::ID)
    }

    #[inline(always)]
    pub fn seeds(handle_hash: &[u8; 32]) -> [&[u8]; 2] {
        [Self::SEED, handle_hash.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(handle_hash: &'a [u8; 32], bump: &'a [u8]) -> [&'a [u8]; 3] {
        [Self::SEED, handle_hash.as_ref(), bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(handle_hash: &'a [u8; 32], bump: &'a [u8]) -> [Seed<'a>; 3] {
        [
            Seed::from(Self::SEED),
            Seed::from(handle_hash.as_ref()),
            Seed::from(bump),
        ]
    }

    #[inline(always)]
    pub fn validate_pda(&self, address_of_self: &Address) -> Result<(), ProgramError> {
        require!(self.is_initialized(), ProgramError::InvalidAccountData);

        let derived = Self::derive_pda(&self.handle_hash, self.bump)?;

        require_eq_keys!(&derived, address_of_self, ProgramError::InvalidSeeds);

        Ok(())
    }

    #[inline(always)]
    pub fn handle_bytes(&self) -> &[u8] {
        &self.handle[..self.handle_len as usize]
    }

    #[inline(always)]
    pub fn split_across_keys(&self) -> bool {
        StealthPoolFlags::SplitAcrossKeys.is_in(self.flags)
    }
}

///
/// BitMask Flags
///
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StealthPoolFlags {
    Empty = 0x00, // means no flags
    SplitAcrossKeys = 0x01,
}

impl StealthPoolFlags {
    pub const MASK: u8 = StealthPoolFlags::Empty.value() | StealthPoolFlags::SplitAcrossKeys.value();

    #[inline(always)]
    pub const fn value(self) -> u8 {
        self as u8
    }

    pub const fn is_in(self, flags: u8) -> bool {
        flags & self.value() != 0
    }

    pub const fn is_valid(flags: u8) -> bool {
        flags & !Self::MASK == 0
    }
}

#[cfg(test)]
mod tests {
    use super::StealthPoolFlags;

    #[test]
    fn stealth_pool_flags_accept_empty_and_known_bits() {
        assert!(StealthPoolFlags::is_valid(StealthPoolFlags::Empty.value()));
        assert!(StealthPoolFlags::is_valid(StealthPoolFlags::SplitAcrossKeys.value()));
        assert!(StealthPoolFlags::is_valid(
            StealthPoolFlags::Empty.value() | StealthPoolFlags::SplitAcrossKeys.value()
        ));
    }

    #[test]
    fn stealth_pool_flags_reject_unknown_bits() {
        assert!(!StealthPoolFlags::is_valid(1 << 1));
        assert!(!StealthPoolFlags::is_valid(
            StealthPoolFlags::SplitAcrossKeys.value() | (1 << 1)
        ));
        assert!(!StealthPoolFlags::is_valid(u8::MAX));
    }

    #[test]
    fn stealth_pool_flags_check_membership_by_bit() {
        assert!(!StealthPoolFlags::SplitAcrossKeys.is_in(StealthPoolFlags::Empty.value()));
        assert!(StealthPoolFlags::SplitAcrossKeys.is_in(StealthPoolFlags::SplitAcrossKeys.value()));
    }
}
