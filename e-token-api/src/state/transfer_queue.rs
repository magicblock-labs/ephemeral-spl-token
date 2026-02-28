use pinocchio::{cpi::Seed, error::ProgramError, Address};

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
    /// Shuttle crank ID.
    pub shuttle_crank_id: Option<i64>,
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

    /// Returns the number of transfers that are ready to be processed and the transfers themselves.
    /// `now` is the current time in seconds, used as a pseudo-random number generator.
    pub fn processed_transfers(
        &mut self,
        now: i64,
    ) -> (usize, [(u64, Address); MAX_PROCESSED_TRANSFERS]) {
        let mut result = [const { (0, Address::new_from_array([0; 32])) }; MAX_PROCESSED_TRANSFERS];

        if self.length == 0 {
            return (0, result);
        }

        fn get_next_index(i: usize, length: usize, now: i64) -> usize {
            i.wrapping_mul(length).wrapping_mul(now as usize) as usize % length
        }

        let mut transfers_to_consider = self.length as usize;
        let mut i = get_next_index(1, transfers_to_consider, now);
        let mut processed_transfers = 0;
        loop {
            if self.queue[i].last_transfer + (self.queue[i].interval_seconds as i64) > now {
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
        (processed_transfers, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: Address = Address::new_from_array([0; 32]);
    const DESTINATION: Address = Address::new_from_array([1; 32]);
    const AMOUNT: u64 = 1000;
    const CHUNK_SIZE: u64 = 100;
    const INTERVAL: u16 = 10;

    fn ready_transfer(now: i64) -> QueuedTransfer {
        QueuedTransfer {
            source: SOURCE,
            destination: DESTINATION,
            amount: AMOUNT,
            chunk_size: CHUNK_SIZE,
            interval_seconds: INTERVAL,
            last_transfer: now - INTERVAL as i64,
        }
    }

    fn not_ready_transfer(now: i64) -> QueuedTransfer {
        QueuedTransfer {
            source: SOURCE,
            destination: DESTINATION,
            amount: AMOUNT,
            chunk_size: CHUNK_SIZE,
            interval_seconds: INTERVAL,
            last_transfer: now,
        }
    }

    #[test]
    fn test_processed_transfers_one_ready() {
        let mut queue = TransferQueue {
            bump: 0,
            mint: Address::new_from_array([0; 32]),
            length: 0,
            shuttle_crank_id: None,
            queue: core::array::from_fn(|_| QueuedTransfer::default()),
        };
        queue.length = 1;
        let now = 10;
        queue.queue[0] = ready_transfer(now);
        let (processed_transfers, transfers) = queue.processed_transfers(now);
        assert_eq!(processed_transfers, 1);
        assert_eq!(transfers[0].0, 100);
        assert_eq!(transfers[0].1, DESTINATION);
    }

    #[test]
    fn test_processed_transfers_queue_full() {
        let mut now = 10;
        let mut queue = TransferQueue {
            bump: 0,
            mint: Address::new_from_array([0; 32]),
            length: MAX_QUEUE_SIZE as u32,
            shuttle_crank_id: None,
            queue: core::array::from_fn(|_| ready_transfer(now)),
        };

        for _ in 0..(AMOUNT.div_ceil(CHUNK_SIZE) * MAX_QUEUE_SIZE as u64) {
            let length = queue.length as usize;
            let (processed_transfers, transfers) = queue.processed_transfers(now);
            assert_eq!(processed_transfers, MAX_PROCESSED_TRANSFERS.min(length));
            assert_eq!(
                transfers.iter().map(|(amount, _)| amount).sum::<u64>(),
                MAX_PROCESSED_TRANSFERS.min(length) as u64 * CHUNK_SIZE
            );
            now += INTERVAL as i64;
        }
        let (processed_transfers, _transfers) = queue.processed_transfers(now);
        assert_eq!(processed_transfers, 0);
    }

    #[test]
    fn test_processed_transfers_queue_full_not_ready() {
        let mut now = 10;
        let mut queue = TransferQueue {
            bump: 0,
            mint: Address::new_from_array([0; 32]),
            length: MAX_QUEUE_SIZE as u32,
            shuttle_crank_id: None,
            queue: core::array::from_fn(|_| not_ready_transfer(now)),
        };

        // No transfers are ready yet
        let (processed_transfers, _transfers) = queue.processed_transfers(now);
        assert_eq!(processed_transfers, 0);
        now += INTERVAL as i64;

        // All transfers are ready now
        for _ in 0..(AMOUNT.div_ceil(CHUNK_SIZE) * MAX_QUEUE_SIZE as u64) {
            let length = queue.length as usize;
            let (processed_transfers, _transfers) = queue.processed_transfers(now);
            assert_eq!(processed_transfers, MAX_PROCESSED_TRANSFERS.min(length));
            now += INTERVAL as i64;
        }
    }

    #[test]
    fn test_finished_processing_transfers_different_amounts() {
        let mut now = 10;
        let mut queue = TransferQueue {
            bump: 0,
            mint: Address::new_from_array([0; 32]),
            length: MAX_PROCESSED_TRANSFERS as u32,
            shuttle_crank_id: None,
            queue: core::array::from_fn(|i| {
                if i == 0 {
                    let mut tranfer = ready_transfer(now);
                    tranfer.amount += CHUNK_SIZE;
                    tranfer
                } else if i < MAX_PROCESSED_TRANSFERS {
                    ready_transfer(now)
                } else {
                    QueuedTransfer::default()
                }
            }),
        };

        for _ in 0..(AMOUNT.div_ceil(CHUNK_SIZE)) {
            let (processed_transfers, _transfers) = queue.processed_transfers(now);
            assert_eq!(processed_transfers, MAX_PROCESSED_TRANSFERS);
            now += INTERVAL as i64;
        }

        // The last transfer will complete
        let (processed_transfers, _transfers) = queue.processed_transfers(now);
        assert_eq!(processed_transfers, 1);
    }
}
