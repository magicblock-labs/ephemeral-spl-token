#![no_std]

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.

pinocchio_pubkey::declare_id!("5iC4wKZizyxrKh271Xzx3W4Vn2xUyYvSGHeoB2mdw5HA");

/// Instruction discriminators for the Ephemeral SPL program.
/// Keep in sync with the on-chain program dispatcher.
pub mod instruction {
    /// 0 - InitializeMint (placeholder, mirrors SPL Token API but handled by this program)
    pub const INITIALIZE_MINT: u8 = 0;
    /// 1 - InitializeEphemeralAta: initialize the ephemeral ATA account derived from [payer, mint]
    pub const INITIALIZE_EPHEMERAL_ATA: u8 = 1;
}

/// Ephemeral Associated Token Account (ATA) data layout.
/// This account is a PDA derived from seeds [payer, mint] under this program ID.
/// For now it only stores a 64-bit balance and is zero-initialized on creation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EphemeralAta {
    /// Current balance tracked by the program.
    pub balance: u64,
}

impl EphemeralAta {
    /// Size of the account data in bytes.
    pub const LEN: usize = core::mem::size_of::<Self>();

    /// Returns a zero-initialized instance.
    pub const fn zero() -> Self {
        Self { balance: 0 }
    }
}
