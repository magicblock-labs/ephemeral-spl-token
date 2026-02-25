pub const QUEUE_SEED: &[u8] = b"queue";

/// The max size of the transfer queue.
/// 127 transfers per queue fits in 10kb, preventing reallocs.
pub const MAX_QUEUE_SIZE: usize = 127;
