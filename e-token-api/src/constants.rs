pub const QUEUE_SEED: &[u8] = b"queue";

/// The max size of the transfer queue.
/// 1000 -> 88kb -> 0.685 SOL
pub const MAX_QUEUE_SIZE: usize = 100;

/// The maximum number of chunks per transfer.
pub const MAX_CHUNKS_PER_TRANSFER: u16 = 100;

/// The maximum duration of a transfer in seconds.
pub const MAX_TRANSFER_DURATION_SECONDS: u16 = 60 * 60 * 12; // 12 hours

/// The maximum number of transfers processed
pub const MAX_PROCESSED_TRANSFERS: usize = 5;
