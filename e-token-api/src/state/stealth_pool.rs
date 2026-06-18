use bytemuck::{Pod, Zeroable};
use pinocchio::error::ProgramError;
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
    // Exact UTF-8 handle bytes used to derive this PDA. Byte 0 stores the
    // handle length, followed by up to 64 handle bytes.
    pub handle: [u8; 65],
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

    pub const MAX_HANDLE_BYTES: usize = 64;
    pub const HANDLE_STORAGE_LEN: usize = 1 + Self::MAX_HANDLE_BYTES;
    pub const MAX_HANDLE_SEED_BYTES: usize = 32;

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
    pub fn handle_from_storage(storage: &[u8; 65]) -> Result<&[u8], ProgramError> {
        let len = storage[0] as usize;
        require!(
            len != 0 && len <= Self::MAX_HANDLE_BYTES,
            ProgramError::InvalidInstructionData
        );

        Ok(&storage[1..1 + len])
    }

    #[inline(always)]
    pub fn store_handle(handle: &[u8]) -> Result<[u8; 65], ProgramError> {
        Self::validate_handle(handle)?;

        let mut storage = [0u8; Self::HANDLE_STORAGE_LEN];
        storage[0] = handle.len() as u8;
        storage[1..1 + handle.len()].copy_from_slice(handle);
        Ok(storage)
    }

    #[inline(always)]
    pub fn derive_pda(handle: &[u8], bump_seed: u8) -> Result<Address, ProgramError> {
        Self::validate_handle(handle)?;
        let bump = [bump_seed];

        if handle.len() <= Self::MAX_HANDLE_SEED_BYTES {
            Ok(Address::create_program_address(
                &[Self::SEED, handle, &bump],
                &crate::ID,
            )?)
        } else {
            Ok(Address::create_program_address(
                &[
                    Self::SEED,
                    &handle[..Self::MAX_HANDLE_SEED_BYTES],
                    &handle[Self::MAX_HANDLE_SEED_BYTES..],
                    &bump,
                ],
                &crate::ID,
            )?)
        }
    }

    #[inline(always)]
    pub fn find_pda(handle: &[u8]) -> Result<(Address, u8), ProgramError> {
        Self::validate_handle(handle)?;

        if handle.len() <= Self::MAX_HANDLE_SEED_BYTES {
            Ok(Address::find_program_address(&[Self::SEED, handle], &crate::ID))
        } else {
            Ok(Address::find_program_address(
                &[
                    Self::SEED,
                    &handle[..Self::MAX_HANDLE_SEED_BYTES],
                    &handle[Self::MAX_HANDLE_SEED_BYTES..],
                ],
                &crate::ID,
            ))
        }
    }

    #[inline(always)]
    pub fn validate_pda(&self, address_of_self: &Address) -> Result<(), ProgramError> {
        require!(self.is_initialized(), ProgramError::InvalidAccountData);

        let derived = Self::derive_pda(self.handle_bytes(), self.bump)?;

        require_eq_keys!(&derived, address_of_self, ProgramError::InvalidSeeds);

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
