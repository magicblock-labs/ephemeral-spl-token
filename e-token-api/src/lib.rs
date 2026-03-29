#![no_std]

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod consts;
pub mod error;
pub mod state;
pub mod program {
    pinocchio_pubkey::declare_id!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");
    pub use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;

    /// Returns the program ID as an Address
    #[inline(always)]
    pub const fn id_address() -> pinocchio::Address {
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
    ///      [8..16]   min_delay_ms (u64 LE), 0 => immediate
    ///      [16..24]  max_delay_ms (u64 LE), must be >= min_delay_ms
    ///      [24..28]  split count (u32 LE), must be >= 1
    pub const DEPOSIT_AND_QUEUE_TRANSFER: u8 = 16;
    /// 17 - EnsureTransferQueueCrank: ensure the per-mint recurring queue crank is scheduled
    ///      Instruction data:
    ///      []
    pub const ENSURE_TRANSFER_QUEUE_CRANK: u8 = 17;
    /// 19 - DelegateTransferQueue: delegate the per-mint transfer queue PDA to the delegation program
    ///      Instruction data:
    ///      []        no instruction args
    pub const DELEGATE_TRANSFER_QUEUE: u8 = 19;
    /// 20 - SponsoredLamportsTransfer: create a zero-data PDA derived from
    ///      [b"lamports", payer, destination, salt], fund it with the requested
    ///      lamports plus sponsored rent from the global rent PDA, delegate it,
    ///      then schedule post-delegation transfer + cleanup actions.
    ///      Instruction data:
    ///      [0..8]   amount (u64 LE)
    ///      [8..40]  salt ([u8; 32])
    ///      [40..72] optional validator pubkey
    pub const SPONSORED_LAMPORTS_TRANSFER: u8 = 20;
    /// 23 - InitializeRentPda: initialize the global rent-sponsoring PDA derived from ["rent"]
    ///      Instruction data:
    ///      []        no instruction args
    pub const INITIALIZE_RENT_PDA: u8 = 23;
    /// 24 - SetupAndDelegateShuttleEphemeralAtaWithMerge: initialize shuttle metadata/EATA/wallet ATA,
    ///      deposit tokens into the shuttle EATA through the global vault, sponsor delegation from
    ///      the global rent PDA, and schedule post-delegation merge + cleanup.
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    shuttle metadata bump
    ///      [5..13] deposit amount (u64 LE)
    ///      [13..45] optional validator pubkey
    pub const SETUP_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE: u8 = 24;
    /// 25 - DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer:
    ///      same setup/deposit/delegate flow as instruction 24, but instead of using instruction 24's
    ///      merge-to-destination behavior, the first post-delegation action restores the owner's
    ///      source token account by merging the shuttle balance back there, then a third post-delegation
    ///      action schedules a private transfer of the same amount to the destination owner's
    ///      canonical ATA.
    ///      SDK callers must account for that queued private transfer when calculating required source
    ///      balances and expected destination credits; the destination does not hold the final funds
    ///      immediately after the merge/cleanup steps.
    ///      Instruction data:
    ///      [0..4]   shuttle_id (u32 LE)
    ///      [4..12]  deposit amount (u64 LE)
    ///      [12..]   len-prefixed optional validator pubkey bytes
    ///      [...]    len-prefixed encrypted destination owner pubkey bytes
    ///      [...]    len-prefixed encrypted packed suffix
    ///               (min_delay_ms:u64, max_delay_ms:u64, split:u32, flags:u8)
    pub const DEPOSIT_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE_AND_PRIVATE_TRANSFER: u8 = 25;
    /// 26 - WithdrawThroughDelegatedShuttleWithMerge: initialize shuttle metadata/EATA/wallet ATA,
    ///      sponsor delegation from the global rent PDA, then schedule a post-delegation transfer
    ///      from the owner ATA into the shuttle wallet ATA followed by shuttle undelegate
    ///      and close/refund.
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    shuttle metadata bump
    ///      [5..13] transfer amount (u64 LE)
    ///      [13..45] optional validator pubkey
    pub const WITHDRAW_THROUGH_DELEGATED_SHUTTLE_WITH_MERGE: u8 = 26;
    /// 27 - AllocateTransferQueue: allocates more space for the transfer queue
    pub const ALLOCATE_TRANSFER_QUEUE: u8 = 27;

    /// Internal-only instruction discriminators used by the on-chain program.
    pub mod internal {
        /// 196 - UndelegationCallback: delegation-program callback used to restore delegated state.
        pub const UNDELEGATION_CALLBACK: u8 = 196;
        /// 197 - CloseShuttleAtaIntent: Magic standalone action that closes an emptied shuttle ATA flow.
        pub const CLOSE_SHUTTLE_ATA_INTENT: u8 = 197;
        /// 198 - ExecuteReadyQueuedTransfer: Magic standalone action that settles one queued transfer.
        pub const EXECUTE_READY_QUEUED_TRANSFER: u8 = 198;
        /// 199 - ProcessTransferQueueTick: recurring crank callback that checks a queue and schedules settlement.
        pub const PROCESS_TRANSFER_QUEUE_TICK: u8 = 199;
        /// 200 - TransferLamportsPda: post-delegation action that transfers the
        ///       requested lamports from the delegated zero-data PDA to the
        ///       destination base-layer account.
        pub const TRANSFER_LAMPORTS_PDA: u8 = 200;
        /// 201 - UndelegateWithdrawAndCloseShuttleEphemeralAta: internal post-delegation action that
        ///       undelegates a shuttle and schedules the close/refund post-undelegate action.
        pub const UNDELEGATE_WITHDRAW_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA: u8 = 201;
        /// 202 - UndelegateLamportsPda: post-delegation action that commits and
        ///       undelegates the lamports PDA, then schedules the close/refund intent.
        pub const UNDELEGATE_LAMPORTS_PDA: u8 = 202;
        /// 203 - CloseLamportsPdaIntent: post-undelegate Magic intent that
        ///       refunds the sponsored rent to the global rent PDA and closes the PDA.
        pub const CLOSE_LAMPORTS_PDA_INTENT: u8 = 203;
    }
}
