use bytemuck::{Pod, Zeroable};
use const_crypto::sha2::Sha256;
use pinocchio::error::ProgramError;
use solana_address::{address_eq, Address};

use crate::{
    require,
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
    // Exact UTF-8 handle bytes used to derive this PDA. Byte 0 stores the
    // handle length, followed by up to 255 handle bytes.
    pub handle: [u8; 256],
    pub destination_count: u8,
    pub destinations: [Address; 10],
}

impl Initializable for StealthPool {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.discriminator == StealthPool::DISCRIMINATOR
            && self.handle[0] != 0
            && self.handle[0] as usize <= StealthPool::MAX_HANDLE_BYTES
            && self.destination_count != 0
            && self.destination_count as usize <= StealthPool::MAX_DESTINATIONS
    }
}

impl RawType for StealthPool {
    const LEN: usize = core::mem::size_of::<StealthPool>();
}

impl StealthPool {
    // The discriminator has name + version
    pub const DISCRIMINATOR: [u8; 8] = *b"stpool@1";

    pub const SEED: &'static [u8] = b"stealth_pool";
    pub const LONG_HANDLE_HASH_DOMAIN: &'static [u8] = b"stealth_pool_handle";

    pub const MAX_HANDLE_BYTES: usize = u8::MAX as usize;
    pub const HANDLE_STORAGE_LEN: usize = 1 + Self::MAX_HANDLE_BYTES;
    pub const MAX_HANDLE_SEED_BYTES: usize = 32;
    pub const MAX_INLINE_HANDLE_SEED_BYTES: usize = 2 * Self::MAX_HANDLE_SEED_BYTES;
    pub const MAX_PDA_SEEDS: usize = 3;
    pub const MAX_PDA_SIGNER_SEEDS: usize = Self::MAX_PDA_SEEDS + 1;

    pub const MAX_DESTINATIONS: usize = 10;

    #[inline(always)]
    pub fn validate_handle(handle: &[u8]) -> Result<(), ProgramError> {
        require!(
            !handle.is_empty() && handle.len() <= Self::MAX_HANDLE_BYTES,
            ProgramError::InvalidInstructionData
        );

        Ok(())
    }

    #[inline(always)]
    pub fn handle_from_storage(storage: &[u8; Self::HANDLE_STORAGE_LEN]) -> Result<&[u8], ProgramError> {
        let len = storage[0] as usize;
        require!(
            len != 0 && len <= Self::MAX_HANDLE_BYTES,
            ProgramError::InvalidInstructionData
        );

        Ok(&storage[1..1 + len])
    }

    #[inline(always)]
    pub fn store_handle(handle: &[u8]) -> Result<[u8; Self::HANDLE_STORAGE_LEN], ProgramError> {
        Self::validate_handle(handle)?;

        let mut storage = [0u8; Self::HANDLE_STORAGE_LEN];
        storage[0] = handle.len() as u8;
        storage[1..1 + handle.len()].copy_from_slice(handle);
        Ok(storage)
    }

    #[inline(always)]
    pub fn long_handle_hash_seed(handle: &[u8]) -> Result<[u8; 32], ProgramError> {
        Self::validate_handle(handle)?;

        Ok(Sha256::new()
            .update(Self::LONG_HANDLE_HASH_DOMAIN)
            .update(handle)
            .finalize())
    }

    #[inline(always)]
    pub fn seed_slices<'a>(
        handle: &'a [u8],
        long_handle_hash_seed: &'a mut [u8; 32],
        out: &'a mut [&'a [u8]; Self::MAX_PDA_SEEDS],
    ) -> Result<&'a [&'a [u8]], ProgramError> {
        let hash_seed = if handle.len() > Self::MAX_INLINE_HANDLE_SEED_BYTES {
            *long_handle_hash_seed = Self::long_handle_hash_seed(handle)?;
            Some(&*long_handle_hash_seed)
        } else {
            None
        };
        Self::seed_slices_with_hash(handle, hash_seed, out)
    }

    #[inline(always)]
    pub fn seed_slices_with_hash<'a>(
        handle: &'a [u8],
        long_handle_hash_seed: Option<&'a [u8; 32]>,
        out: &'a mut [&'a [u8]; Self::MAX_PDA_SEEDS],
    ) -> Result<&'a [&'a [u8]], ProgramError> {
        Self::validate_handle(handle)?;

        out[0] = Self::SEED;
        if handle.len() <= Self::MAX_HANDLE_SEED_BYTES {
            out[1] = handle;
            return Ok(&out[..2]);
        }

        out[1] = &handle[..Self::MAX_HANDLE_SEED_BYTES];
        if handle.len() <= Self::MAX_INLINE_HANDLE_SEED_BYTES {
            out[2] = &handle[Self::MAX_HANDLE_SEED_BYTES..];
        } else {
            let hash_seed = long_handle_hash_seed.ok_or(ProgramError::InvalidInstructionData)?;
            let hash_seed: &'a [u8] = &hash_seed[..];
            out[2] = hash_seed;
        }

        Ok(&out[..3])
    }

    #[inline(always)]
    pub fn seed_slices_with_bump<'a>(
        handle: &'a [u8],
        bump: &'a [u8],
        long_handle_hash_seed: &'a mut [u8; 32],
        out: &'a mut [&'a [u8]; Self::MAX_PDA_SIGNER_SEEDS],
    ) -> Result<&'a [&'a [u8]], ProgramError> {
        let hash_seed = if handle.len() > Self::MAX_INLINE_HANDLE_SEED_BYTES {
            *long_handle_hash_seed = Self::long_handle_hash_seed(handle)?;
            Some(&*long_handle_hash_seed)
        } else {
            None
        };
        Self::seed_slices_with_bump_and_hash(handle, bump, hash_seed, out)
    }

    #[inline(always)]
    pub fn seed_slices_with_bump_and_hash<'a>(
        handle: &'a [u8],
        bump: &'a [u8],
        long_handle_hash_seed: Option<&'a [u8; 32]>,
        out: &'a mut [&'a [u8]; Self::MAX_PDA_SIGNER_SEEDS],
    ) -> Result<&'a [&'a [u8]], ProgramError> {
        Self::validate_handle(handle)?;

        out[0] = Self::SEED;
        if handle.len() <= Self::MAX_HANDLE_SEED_BYTES {
            out[1] = handle;
            out[2] = bump;
            return Ok(&out[..3]);
        }

        out[1] = &handle[..Self::MAX_HANDLE_SEED_BYTES];
        if handle.len() <= Self::MAX_INLINE_HANDLE_SEED_BYTES {
            out[2] = &handle[Self::MAX_HANDLE_SEED_BYTES..];
        } else {
            let hash_seed = long_handle_hash_seed.ok_or(ProgramError::InvalidInstructionData)?;
            let hash_seed: &'a [u8] = &hash_seed[..];
            out[2] = hash_seed;
        }
        out[3] = bump;

        Ok(&out[..4])
    }

    #[inline(always)]
    pub fn derive_pda(handle: &[u8], bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let mut long_handle_hash_seed = [0u8; 32];
        let mut seed_buf: [&[u8]; Self::MAX_PDA_SIGNER_SEEDS] = [&[]; Self::MAX_PDA_SIGNER_SEEDS];
        let seeds = Self::seed_slices_with_bump(handle, &bump, &mut long_handle_hash_seed, &mut seed_buf)?;

        Ok(Address::create_program_address(seeds, &crate::ID)?)
    }

    #[inline(always)]
    pub fn derive_pda_with_hash(
        handle: &[u8],
        bump_seed: u8,
        long_handle_hash_seed: Option<&[u8; 32]>,
    ) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let mut seed_buf: [&[u8]; Self::MAX_PDA_SIGNER_SEEDS] = [&[]; Self::MAX_PDA_SIGNER_SEEDS];
        let seeds = Self::seed_slices_with_bump_and_hash(handle, &bump, long_handle_hash_seed, &mut seed_buf)?;

        Ok(Address::create_program_address(seeds, &crate::ID)?)
    }

    #[inline(always)]
    pub fn find_pda(handle: &[u8]) -> Result<(Address, u8), ProgramError> {
        let mut long_handle_hash_seed = [0u8; 32];
        let mut seed_buf: [&[u8]; Self::MAX_PDA_SEEDS] = [&[]; Self::MAX_PDA_SEEDS];
        let seeds = Self::seed_slices(handle, &mut long_handle_hash_seed, &mut seed_buf)?;

        Ok(Address::find_program_address(seeds, &crate::ID))
    }

    #[inline(always)]
    pub fn find_pda_with_hash(
        handle: &[u8],
        long_handle_hash_seed: Option<&[u8; 32]>,
    ) -> Result<(Address, u8), ProgramError> {
        let mut seed_buf: [&[u8]; Self::MAX_PDA_SEEDS] = [&[]; Self::MAX_PDA_SEEDS];
        let seeds = Self::seed_slices_with_hash(handle, long_handle_hash_seed, &mut seed_buf)?;

        Ok(Address::find_program_address(seeds, &crate::ID))
    }

    #[inline(always)]
    pub fn validate_pda(&self, address_of_self: &Address) -> Result<(), ProgramError> {
        require!(self.is_initialized(), ProgramError::InvalidAccountData);

        let derived = Self::derive_pda(self.handle_bytes(), self.bump)?;

        require!(address_eq(&derived, address_of_self), ProgramError::InvalidSeeds);

        Ok(())
    }

    #[inline(always)]
    pub fn handle_bytes(&self) -> &[u8] {
        let len = self.handle[0] as usize;
        if len <= Self::MAX_HANDLE_BYTES {
            &self.handle[1..1 + len]
        } else {
            &[]
        }
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
    use super::{StealthPool, StealthPoolFlags};

    #[test]
    fn stealth_pool_handle_storage_accepts_255_bytes() {
        let handle = [b'a'; StealthPool::MAX_HANDLE_BYTES];
        let storage = StealthPool::store_handle(&handle).unwrap();

        assert_eq!(storage[0] as usize, StealthPool::MAX_HANDLE_BYTES);
        assert_eq!(&storage[1..], &handle);
        assert_eq!(StealthPool::handle_from_storage(&storage).unwrap(), &handle);
    }

    #[test]
    fn stealth_pool_handle_storage_rejects_more_than_255_bytes() {
        let handle = [b'a'; StealthPool::MAX_HANDLE_BYTES + 1];

        assert!(StealthPool::store_handle(&handle).is_err());
        assert!(StealthPool::find_pda(&handle).is_err());
    }

    #[test]
    fn stealth_pool_pda_seeds_hash_255_byte_handles() {
        let handle = [b'a'; StealthPool::MAX_HANDLE_BYTES];
        let expected_hash = StealthPool::long_handle_hash_seed(&handle).unwrap();
        let mut long_handle_hash_seed = [0u8; 32];
        let mut seeds = [&[][..]; StealthPool::MAX_PDA_SEEDS];
        let seeds = StealthPool::seed_slices(&handle, &mut long_handle_hash_seed, &mut seeds).unwrap();

        assert_eq!(seeds.len(), StealthPool::MAX_PDA_SEEDS);
        assert_eq!(seeds[0], StealthPool::SEED);
        assert_eq!(seeds[1], &handle[..StealthPool::MAX_HANDLE_SEED_BYTES]);
        assert_eq!(seeds[2], &expected_hash);
    }

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
