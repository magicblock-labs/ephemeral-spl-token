use pinocchio::{cpi::Seed, Address};

use crate::constants::{MAX_QUEUE_SIZE, QUEUE_SEED};

use super::{Initializable, RawType};

/// Internal representation of a queued transfer.
#[repr(C)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct QueuedTransfer {
    /// The source address.
    pub source: Address,
    /// The destination address.
    pub destination: Address,
    /// The amount of tokens to transfer.
    pub amount: u64,
    /// The max amount transferred in a single transfer.
    pub chunk_size: u64,
}

/// Internal representation of a transfer queue.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferQueue {
    /// The canonical bump of the queue.
    pub bump: u8,
    /// The mint associated with the queue.
    pub mint: Address,
    /// The queue length.
    pub length: u8,
    /// The queue of transfers.
    pub queue: [QueuedTransfer; MAX_QUEUE_SIZE],
}

impl RawType for TransferQueue {
    const LEN: usize = core::mem::size_of::<TransferQueue>();
}

impl Initializable for TransferQueue {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Address::default()
    }
}

impl TransferQueue {
    pub fn seeds(mint: &Address) -> [&[u8]; 2] {
        [QUEUE_SEED.as_ref(), mint.as_ref()]
    }

    pub fn signer_seeds(mint: &Address) -> [Seed; 2] {
        [Seed::from(QUEUE_SEED), Seed::from(mint.as_ref())]
    }

    pub fn seeds_with_bump<'a>(mint: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 3] {
        [
            Seed::from(QUEUE_SEED),
            Seed::from(mint.as_ref()),
            Seed::from(bump),
        ]
    }

    pub fn find_pda(mint: &Address) -> (Address, u8) {
        Address::find_program_address(&TransferQueue::seeds(&mint), &crate::program::id_address())
    }
}
