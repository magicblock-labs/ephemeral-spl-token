use pinocchio::{
    cpi::Seed,
    error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    Address,
};

use crate::constants::{MAX_PROCESSED_TRANSFERS, MAX_QUEUE_SIZE, QUEUE_SEED};

use super::{Initializable, RawType};

/// Internal representation of a queued transfer.
#[repr(C)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct QueuedTransfer {
    /// The source address.
    pub source: Address,
    /// The destination address.
    /// Stored to recover the tokens if the ER shuts down.
    pub destination: Address,
    /// The amount of tokens to transfer.
    pub amount: u64,
    /// The max amount transferred in a single transfer.
    pub chunk_size: u64,
    /// The interval in seconds between transfers.
    pub interval_seconds: u16,
    /// The timestamp of the last transfer.
    pub last_transfer: i64,
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
    pub length: u32,
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
    #[inline(always)]
    pub fn create_address(mint: &Address, bump: &[u8]) -> Result<Address, ProgramError> {
        Address::create_program_address(
            &TransferQueue::seeds_with_bump(mint, bump),
            &crate::program::ID,
        )
        .map_err(|_| ProgramError::InvalidSeeds)
    }

    #[inline(always)]
    pub fn find_pda(mint: &Address) -> (Address, u8) {
        Address::find_program_address(&TransferQueue::seeds(&mint), &crate::program::ID)
    }

    pub fn seeds(mint: &Address) -> [&[u8]; 2] {
        [QUEUE_SEED.as_ref(), mint.as_ref()]
    }

    pub fn seeds_with_bump<'a>(mint: &'a Address, bump: &'a [u8]) -> [&'a [u8]; 3] {
        [QUEUE_SEED.as_ref(), mint.as_ref(), bump]
    }

    pub fn signer_seeds<'a>(mint: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 3] {
        [
            Seed::from(QUEUE_SEED),
            Seed::from(mint.as_ref()),
            Seed::from(bump),
        ]
    }

    pub fn processed_transfers(
        &mut self,
    ) -> Result<(usize, [(u64, Address); MAX_PROCESSED_TRANSFERS]), ProgramError> {
        let mut result = [const { (0, Address::new_from_array([0; 32])) }; MAX_PROCESSED_TRANSFERS];
        // Using the clock as a pseudo-random number generator.
        let now = Clock::get()?.unix_timestamp;

        fn get_next_index(i: usize, length: usize, now: i64) -> usize {
            i.wrapping_mul(now as usize) as usize % length
        }

        let mut transfers_to_consider = self.length as usize;
        let mut i = get_next_index(1, transfers_to_consider, now);
        let mut processed_transfers = 0;
        loop {
            if self.queue[i].last_transfer + (self.queue[i].interval_seconds as i64) < now {
                // Transfer is not ready yet.
                // We put it at the end of the queue and pick a new one.
                self.queue.swap(i, self.length as usize - 1);
                transfers_to_consider -= 1;
            } else {
                // Transfer is ready.
                let initial_amount = self.queue[i].amount;
                let amount_to_transfer = initial_amount.min(self.queue[i].chunk_size);
                let destination = self.queue[i].destination.clone();
                self.queue[i].amount -= self.queue[i].chunk_size;
                self.queue[i].last_transfer = now;

                if amount_to_transfer == initial_amount {
                    // Transfer is complete
                    // Remove it by putting it after the last element
                    self.queue.swap(i, self.length as usize - 1);
                    self.length -= 1;
                } else {
                    // Put it at the end of the queue to stop considering it
                    self.queue.swap(i, transfers_to_consider - 1);
                };

                result[processed_transfers] = (amount_to_transfer, destination);

                transfers_to_consider -= 1;
                processed_transfers += 1;
                if processed_transfers >= MAX_PROCESSED_TRANSFERS {
                    break;
                }
            }

            if transfers_to_consider == 0 {
                // Not enough transfers are ready yet.
                break;
            }

            i = get_next_index(i, transfers_to_consider, now);
        }

        // The last N elements
        Ok((processed_transfers, result))
    }
}
