use solana_address::Address;

use super::RawType;

pub const QUEUE_REFILL_STATE_SEED: &[u8] = b"queue-refill";

/// `TransferQueueRefillState` is an existence-only PDA marker. It stores no
/// fields, and its `RawType` impl sets `LEN` to `0`, so the program uses
/// account existence rather than serialized data.
#[repr(C)]
pub struct TransferQueueRefillState {}

impl RawType for TransferQueueRefillState {
    const LEN: usize = 0;
}

#[inline(always)]
pub fn derive_transfer_queue_refill_state_address(queue: &Address) -> (Address, u8) {
    Address::find_program_address(&[QUEUE_REFILL_STATE_SEED, queue.as_ref()], &crate::ID)
}
