#![no_std]
extern crate alloc;

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod consts;
pub mod error;
pub mod instruction;
pub mod requires;
pub mod state;
pub mod program {
    pub use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
}

solana_address::declare_id!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");

/// Re-exported `Address` type from solana-address for convenience.
/// Used throughout the codebase for account address representations.
pub use solana_address::Address;
