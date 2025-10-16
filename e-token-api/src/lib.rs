#![no_std]

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod error;
pub mod state;
pub mod program {
    pinocchio_pubkey::declare_id!("5iC4wKZizyxrKh271Xzx3W4Vn2xUyYvSGHeoB2mdw5HA");
}

/// Instruction discriminators for the Ephemeral SPL program.
/// Keep in sync with the on-chain program dispatcher.
pub mod instruction {
    /// 0 - InitializeEphemeralAta: initialize the ephemeral ATA account derived from [payer, mint]
    pub const INITIALIZE_EPHEMERAL_ATA: u8 = 0;
    /// 1 - InitializeGlobalVault: initialize the global vault account derived from [mint]
    pub const INITIALIZE_GLOBAL_VAULT: u8 = 1;
    /// 2 - DepositSplTokens: transfer tokens to global vault and increase EphemeralAta amount
    pub const DEPOSIT_SPL_TOKENS: u8 = 2;
}
