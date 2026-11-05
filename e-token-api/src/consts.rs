/// Flat lamport fee charged by sponsored shuttle delegation setup flows.
///
/// 0.003 SOL = 3_000_000 lamports. Matches the delegation program's
/// per-session fee (`SESSION_FEE_LAMPORTS`), which it deducts from the
/// sponsored delegation rent when the shuttle is undelegated.
pub const SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS: u64 = if cfg!(feature = "no-fees") { 0 } else { 3_000_000 };

/// Additional flat lamport fee charged by sponsored shuttle delegation setup
/// flows that also enqueue a private transfer.
///
/// 0.00153928 SOL = 1_539_280 lamports. Combined with
/// `SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS`, the total setup fee is
/// 0.00453928 SOL. It prepays one recipient ATA creation on the base chain at
/// the current rent, which `DepositAndQueueTransfer` grants to the
/// first-delivered split of the private transfer.
pub const SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS: u64 =
    if cfg!(feature = "no-fees") { 0 } else { 1_539_280 };

/// Additional flat lamport fee charged by sponsored shuttle delegation setup
/// flows that merge into an encrypted destination on the ER (instruction 34).
///
/// Covers the ER-side Magic ATA creation and shuttle merge CPIs.
pub const SPONSORED_SHUTTLE_MERGE_TO_ENCRYPTED_DESTINATION_EXTRA_LAMPORTS: u64 =
    if cfg!(feature = "no-fees") { 0 } else { 1_539_280 };

/// Private transfer fee charged on instruction 25 (`DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer`).
///
/// 10 basis points = 0.1%.
pub const PRIVATE_TRANSFER_FEE_BASIS_POINTS: u64 = if cfg!(feature = "no-fees") { 0 } else { 10 };
pub const BASIS_POINTS_FACTOR: u128 = 10_000;

/// Flat lamport fee charged by sponsored lamports transfer flows.
///
/// 0.003 SOL = 3_000_000 lamports. Matches the delegation program's
/// per-session fee (`SESSION_FEE_LAMPORTS`), which it deducts from the
/// sponsored delegation rent when the lamports PDA is undelegated.
pub const SPONSORED_LAMPORTS_TRANSFER_SETUP_LAMPORTS: u64 = if cfg!(feature = "no-fees") { 0 } else { 3_000_000 };

/// Fixed lamports amount used when refilling transfer queues.
///
/// 0.02 SOL = 20_000_000 lamports.
pub const TRANSFER_QUEUE_REFILL_LAMPORTS: u64 = 20_000_000;

/// Upfront lamports buffer funded into a newly initialized transfer queue.
///
/// 0.1 SOL = 100_000_000 lamports. This covers about 1,000 commits at
/// 0.0001 SOL each, which keeps the relationship to
/// `TRANSFER_QUEUE_REFILL_LAMPORTS` discoverable.
pub const TRANSFER_QUEUE_INITIAL_BUFFER_LAMPORTS: u64 = 100_000_000;
