pub const QUEUE_SEED: &[u8] = b"queue";

/// The max size of the transfer queue.
/// 127 transfers per queue fits in 10kb, preventing reallocs.
pub const MAX_QUEUE_SIZE: usize = 115;

/// The minimum chunk size in basis points.
pub const MIN_CHUNK_SIZE_BPS: u16 = 100;

/// The maximum duration of a transfer in seconds.
pub const MAX_TRANSFER_DURATION_SECONDS: u16 = 60 * 60 * 12; // 12 hours
