#![no_std]

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod error;
pub mod state;
pub mod program {
    pinocchio_pubkey::declare_id!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");
    pub use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;

    /// Returns the program ID as an Address
    #[inline(always)]
    pub fn id_address() -> pinocchio::Address {
        pinocchio::Address::new_from_array(ID)
    }
}

/// Re-exported `Address` type from solana-address for convenience.
/// Used throughout the codebase for account address representations.
pub use solana_address::Address;

/// Instruction discriminators for the Ephemeral SPL program.
/// Keep in sync with the on-chain program dispatcher.
pub mod instruction {
    /// 0 - InitializeEphemeralAta: initialize the ephemeral ATA account derived from [user, mint]
    pub const INITIALIZE_EPHEMERAL_ATA: u8 = 0;
    /// 1 - InitializeGlobalVault: initialize the global vault account derived from [mint]
    pub const INITIALIZE_GLOBAL_VAULT: u8 = 1;
    /// 2 - DepositSplTokens: transfer tokens to global vault and increase EphemeralAta amount
    pub const DEPOSIT_SPL_TOKENS: u8 = 2;
    /// 3 - WithdrawSplTokens: transfer tokens from global vault back to user and decrease EphemeralAta amount
    pub const WITHDRAW_SPL_TOKENS: u8 = 3;
    /// 4 - DelegateEphemeralAta: delegate the ephemeral ATA to a DLP program using PDA seeds
    pub const DELEGATE_EPHEMERAL_ATA: u8 = 4;
    /// 5 - UndelegateEphemeralAta: commit state and undelegate an ephemeral ATA via the delegation program
    pub const UNDELEGATE_EPHEMERAL_ATA: u8 = 5;
    /// 6 - CreateEphemeralAtaPermission: create a permission account for the ephemeral ATA
    ///     Instruction data:
    ///     [0] bump
    ///     [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    pub const CREATE_EPHEMERAL_ATA_PERMISSION: u8 = 6;
    /// 7 - DelegateEphemeralAtaPermission: delegate the permission PDA for an ephemeral ATA
    pub const DELEGATE_EPHEMERAL_ATA_PERMISSION: u8 = 7;
    /// 8 - UndelegateEphemeralAtaPermission: commit and undelegate the permission PDA
    pub const UNDELEGATE_EPHEMERAL_ATA_PERMISSION: u8 = 8;
    /// 9 - ResetEphemeralAtaPermission: reset permission members to creation-time defaults
    ///     Instruction data:
    ///     [0] bump
    ///     [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    pub const RESET_EPHEMERAL_ATA_PERMISSION: u8 = 9;
}
