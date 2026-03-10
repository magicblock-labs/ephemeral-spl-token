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
    /// 1 - InitializeGlobalVault: initialize the global vault [mint] PDA plus vault-owned Ephemeral ATA and vault ATA
    pub const INITIALIZE_GLOBAL_VAULT: u8 = 1;
    /// 2 - DepositSplTokens: transfer tokens to global vault and increase EphemeralAta amount
    ///     Works for both standard EATA and shuttle EATA, as long as the data account is program-owned.
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
    /// 10 - CloseEphemeralAta: close an empty ephemeral ATA and refund rent to recipient
    pub const CLOSE_EPHEMERAL_ATA: u8 = 10;
    /// 11 - InitializeShuttleEphemeralAta: initialize shuttle account derived from [owner, mint, shuttle_id]
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    bump
    pub const INITIALIZE_SHUTTLE_EPHEMERAL_ATA: u8 = 11;
    /// 12 - InitializeTransferQueue: initialize per-mint transfer queue PDA derived from [QUEUE_SEED, mint]
    ///      Instruction data:
    ///      []        default size (9728 bytes)
    ///      [0..4]    optional queue size in bytes (u32 LE), 0 => default
    pub const INITIALIZE_TRANSFER_QUEUE: u8 = 12;
    /// 13 - DelegateShuttleEphemeralAta: delegate shuttle account to a DLP program using PDA seeds
    pub const DELEGATE_SHUTTLE_EPHEMERAL_ATA: u8 = 13;
    /// 14 - UndelegateShuttleEphemeralAta: revoke delegation on shuttle ATA and close it when empty
    pub const UNDELEGATE_SHUTTLE_EPHEMERAL_ATA: u8 = 14;
    /// 15 - MergeShuttleIntoEphemeralAta: transfer all shuttle ATA funds into destination ATA and keep shuttle account open
    pub const MERGE_SHUTTLE_INTO_EPHEMERAL_ATA: u8 = 15;
    /// 16 - DepositAndQueueTransfer: transfer tokens from signer into the vault ATA and enqueue one or more delayed transfers
    ///      Instruction data:
    ///      [0..8]    amount (u64 LE)
    ///      [8..16]   delay_seconds (u64 LE), 0 => immediate
    ///      [16..20]  split count (u32 LE), must be >= 1
    pub const DEPOSIT_AND_QUEUE_TRANSFER: u8 = 16;
    /// 17 - EnsureTransferQueueCrank: ensure the per-mint recurring queue crank is scheduled
    ///      Instruction data:
    ///      []
    pub const ENSURE_TRANSFER_QUEUE_CRANK: u8 = 17;
    /// 19 - DelegateTransferQueue: delegate the per-mint transfer queue PDA to the delegation program
    ///      Instruction data:
    ///      []        no instruction args
    pub const DELEGATE_TRANSFER_QUEUE: u8 = 19;

    /// Internal-only instruction discriminators used by the on-chain program.
    pub mod internal {
        /// 196 - UndelegationCallback: delegation-program callback used to restore delegated state.
        pub const UNDELEGATION_CALLBACK: u8 = 196;
        /// 197 - CloseShuttleAtaIntentV2: Magic standalone action that closes an emptied shuttle ATA flow.
        pub const CLOSE_SHUTTLE_ATA_INTENT_V2: u8 = 197;
        /// 198 - ExecuteReadyQueuedTransfer: Magic standalone action that settles one queued transfer.
        pub const EXECUTE_READY_QUEUED_TRANSFER: u8 = 198;
        /// 199 - ProcessTransferQueueTick: recurring crank callback that checks a queue and schedules settlement.
        pub const PROCESS_TRANSFER_QUEUE_TICK: u8 = 199;
    }
}
