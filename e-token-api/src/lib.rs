#![no_std]

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod consts;
pub mod error;
pub mod requires;
pub mod state;
pub mod program {
    pub use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
}

solana_address::declare_id!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");

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
    /// 14 - UndelegateAndCloseShuttleToOwner: revoke delegation on a shuttle ATA
    ///      and schedule settlement/close using an owner-owned destination token account.
    pub const UNDELEGATE_AND_CLOSE_SHUTTLE_TO_OWNER: u8 = 14;
    /// 15 - MergeShuttleIntoEphemeralAta: transfer all shuttle ATA funds into destination ATA and keep shuttle account open
    pub const MERGE_SHUTTLE_INTO_EPHEMERAL_ATA: u8 = 15;
    /// 16 - DepositAndQueueTransfer: transfer tokens from signer into the vault ATA and enqueue one or more delayed transfers
    ///      Instruction data:
    ///      [0..8]    amount (u64 LE)
    ///      [8..16]   min_delay_ms (u64 LE), 0 => immediate
    ///      [16..24]  max_delay_ms (u64 LE), must be >= min_delay_ms
    ///      [24..28]  split count (u32 LE), must be >= 1
    ///      [28]      optional legacy flags (u8)
    ///      [28..36]  optional client_ref_id (u64 LE) when flags are omitted
    ///      [29..37]  optional client_ref_id (u64 LE) after legacy flags
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
    ///               (min_delay_ms:u64, max_delay_ms:u64, split:u32, client_ref_id?:u64)
    ///               Legacy payloads may still append flags before client_ref_id.
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
    /// 28 - ProcessPendingTransferQueueRefill: permissionless idempotent helper that
    ///      checks the queue-refill-state PDA and, when pending, tops up the queue
    ///      lamports from the global rent PDA.
    ///      Instruction data:
    ///      []        no instruction args
    pub const PROCESS_PENDING_TRANSFER_QUEUE_REFILL: u8 = 28;
    /// 29 - ProcessScheduledPrivateTransfer: top-level callback fired by the
    ///      Hydra scheduler. Permissionless, no signer metas (Hydra forbids
    ///      them). Re-derives the stash PDA from [b"stash", user, mint] and
    ///      self-CPIs into instruction 25 using `invoke_signed` so the PDA
    ///      signs for both the `payer` and `owner` slots.
    ///      Instruction data:
    ///      [0..32]  user pubkey (stash PDA seed)
    ///      [32]     stash PDA bump
    ///      [33..37] shuttle_id (u32 LE)
    ///      [37..]   len-prefixed optional validator pubkey
    ///      [...]    len-prefixed encrypted destination owner pubkey
    ///      [...]    len-prefixed encrypted packed suffix (same format as ix 25)
    pub const PROCESS_SCHEDULED_PRIVATE_TRANSFER: u8 = 29;
    /// 30 - SchedulePrivateTransfer: small user-signed ix that creates the
    ///      stash PDA on first use, funds it + a Hydra crank from the global
    ///      rent PDA, and CPIs into `hydra::Create` with a one-shot crank that
    ///      will fire `PROCESS_SCHEDULED_PRIVATE_TRANSFER` as soon as possible.
    ///      Designed to be appended to a swap tx where the swap's
    ///      `destinationTokenAccount` is the stash ATA of `(user, mint)`.
    ///      Keeps the outer-tx footprint tight: the 14 pubkeys that ix 25
    ///      will need at trigger time are derived on-chain from client-
    ///      supplied bumps + hard-coded program IDs, so the caller passes
    ///      only 7 accounts.
    ///      Accounts:
    ///      [0] user (signer), [1] stash_pda (w), [2] rent_pda (w),
    ///      [3] hydra_crank_pda (w), [4] hydra_program,
    ///      [5] system_program, [6] token_program.
    ///      Instruction data:
    ///      [0..4]   shuttle_id (u32 LE)
    ///      [4]      stash_pda bump
    ///      [5..37]  mint pubkey (32 B)
    ///      [37]     shuttle bump
    ///      [38]     shuttle_eata bump
    ///      [39]     shuttle_wallet_ata bump
    ///      [40]     buffer bump
    ///      [41]     delegation_record bump
    ///      [42]     delegation_metadata bump
    ///      [43]     global_vault bump
    ///      [44]     vault_token bump
    ///      [45]     stash_ata bump
    ///      [46]     queue bump
    ///      [47..]   len-prefixed optional validator pubkey (0 or 32)
    ///      [...]    len-prefixed encrypted destination owner pubkey
    ///      [...]    len-prefixed encrypted packed suffix (same format as ix 25)
    pub const SCHEDULE_PRIVATE_TRANSFER: u8 = 30;

    /// Internal-only instruction discriminators used by the on-chain program.
    pub mod internal {
        /// 196 - UndelegationCallback: delegation-program callback used to restore delegated state.
        pub const UNDELEGATION_CALLBACK: u8 = 196;
        /// 197 - SettleAndCloseShuttleIntent: Magic standalone action that withdraws any
        ///       remaining shuttle balance to the supplied destination token account, then
        ///       closes the shuttle accounts.
        pub const SETTLE_AND_CLOSE_SHUTTLE_INTENT: u8 = 197;
        /// 198 - ExecuteReadyQueuedTransfer: Magic standalone action that settles one queued transfer.
        pub const EXECUTE_READY_QUEUED_TRANSFER: u8 = 198;
        /// 199 - ProcessTransferQueueTick: recurring crank callback that checks a queue and schedules settlement.
        pub const PROCESS_TRANSFER_QUEUE_TICK: u8 = 199;
        /// 200 - TransferLamportsPda: post-delegation action that transfers the
        ///       requested lamports from the delegated zero-data PDA to the
        ///       destination base-layer account.
        pub const TRANSFER_LAMPORTS_PDA: u8 = 200;
        /// 202 - UndelegateLamportsPda: post-delegation action that commits and
        ///       undelegates the lamports PDA, then schedules the close/refund intent.
        pub const UNDELEGATE_LAMPORTS_PDA: u8 = 202;
        /// 203 - CloseLamportsPdaIntent: post-undelegate Magic intent that
        ///       refunds the sponsored rent to the global rent PDA and closes the PDA.
        pub const CLOSE_LAMPORTS_PDA_INTENT: u8 = 203;
        /// 204 - MarkTransferQueueRefillPending: Magic standalone action that
        ///       sets the per-queue refill-state pending flag.
        pub const MARK_TRANSFER_QUEUE_REFILL_PENDING: u8 = 204;
    }
}
