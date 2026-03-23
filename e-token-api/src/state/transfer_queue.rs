use bytemuck::{Pod, Zeroable};
use pinocchio::{cpi::Seed, error::ProgramError, Address};

/// Current queue version that stores inserted/ready timestamps in milliseconds.
/// Bump this value only when the on-chain layout changes or queue semantics require it.
pub const TRANSFER_QUEUE_VERSION: u8 = 1;
/// PDA seed prefix for transfer queues.
pub const QUEUE_SEED: &[u8] = b"queue";
pub const QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA: u8 = 1 << 0;

/// Header stored at the start of the queue account.
/// The trailing bytes are interpreted as `[QueuedTransfer]` heap storage.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TransferQueueHeader {
    pub version: u8,
    pub bump: u8,
    pub _pad0: [u8; 6],
    pub mint: Address,
    pub length: u32,
    pub _pad1: [u8; 4],
    pub next_task_id: u64,
    pub crank_task_id: i64,
}

/// One queued transfer entry.
///
/// `ready_at` and `inserted_at` are stored in milliseconds since unix epoch.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct QueuedTransfer {
    pub source: Address,
    pub destination_owner: Address,
    pub amount: u64,
    pub ready_at: i64,
    pub inserted_at: i64,
    pub task_id: u64,
    pub flags: u8,
    pub _pad0: [u8; 7],
}

pub struct TransferQueue;

impl TransferQueue {
    #[inline(always)]
    pub fn create_pda(mint: &Address, bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let pda = Address::create_program_address(&Self::seeds_with_bump(mint, &bump), &crate::ID)?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn find_pda(mint: &Address) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(mint), &crate::ID)
    }

    #[inline(always)]
    pub fn seeds<'a>(mint: &'a Address) -> [&'a [u8]; 2] {
        [QUEUE_SEED, mint.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(mint: &'a Address, bump: &'a [u8]) -> [&'a [u8]; 3] {
        [QUEUE_SEED, mint.as_ref(), bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(mint: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 3] {
        [
            Seed::from(QUEUE_SEED),
            Seed::from(mint.as_ref()),
            Seed::from(bump),
        ]
    }
}

const _: [(); 64] = [(); core::mem::size_of::<TransferQueueHeader>()];
const _: [(); 104] = [(); core::mem::size_of::<QueuedTransfer>()];

impl QueuedTransfer {
    #[inline(always)]
    pub fn should_create_destination_ata_idempotent(&self) -> bool {
        self.flags & QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA != 0
    }
}

#[inline(always)]
pub const fn header_len() -> usize {
    core::mem::size_of::<TransferQueueHeader>()
}

#[inline(always)]
pub const fn item_len() -> usize {
    core::mem::size_of::<QueuedTransfer>()
}

#[inline(always)]
pub fn capacity_from_data_len(data_len: usize) -> usize {
    if data_len < header_len() {
        0
    } else {
        (data_len - header_len()) / item_len()
    }
}

/// Returns typed read-only views over a queue account buffer.
///
/// Returns `(header, full_capacity_items)`, where active heap elements are in
/// `items[..header.length as usize]`.
pub fn queue_views(data: &[u8]) -> Result<(&TransferQueueHeader, &[QueuedTransfer]), ProgramError> {
    if data.len() < header_len() {
        return Err(ProgramError::InvalidAccountData);
    }

    let (header_bytes, rest) = data.split_at(header_len());
    let header = bytemuck::try_from_bytes::<TransferQueueHeader>(header_bytes)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    let cap = rest.len() / item_len();
    let items_bytes = &rest[..cap * item_len()];
    let items = bytemuck::try_cast_slice::<u8, QueuedTransfer>(items_bytes)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    Ok((header, items))
}

/// Returns typed mutable views over a queue account buffer.
///
/// Returns `(header, full_capacity_items)`, where active heap elements are in
/// `items[..header.length as usize]`.
pub fn queue_views_mut(
    data: &mut [u8],
) -> Result<(&mut TransferQueueHeader, &mut [QueuedTransfer]), ProgramError> {
    if data.len() < header_len() {
        return Err(ProgramError::InvalidAccountData);
    }

    let (header_bytes, rest) = data.split_at_mut(header_len());
    let header = bytemuck::try_from_bytes_mut::<TransferQueueHeader>(header_bytes)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    let cap = rest.len() / item_len();
    let items_bytes = &mut rest[..cap * item_len()];
    let items = bytemuck::try_cast_slice_mut::<u8, QueuedTransfer>(items_bytes)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    Ok((header, items))
}

/// Initializes an uninitialized queue (version == 0).
/// Idempotent when already initialized with the same version.
pub fn init_queue(data: &mut [u8], bump: u8, mint: Address) -> Result<(), ProgramError> {
    let (header, items) = queue_views_mut(data)?;
    if items.is_empty() {
        return Err(ProgramError::InvalidAccountData);
    }

    match header.version {
        0 => {
            *header = TransferQueueHeader::zeroed();
            header.version = TRANSFER_QUEUE_VERSION;
            header.bump = bump;
            header.mint = mint;
            Ok(())
        }
        TRANSFER_QUEUE_VERSION => {
            if (header.length as usize) > items.len() {
                return Err(ProgramError::InvalidAccountData);
            }
            Ok(())
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

#[inline(always)]
fn checked_active_len(length: u32, capacity: usize) -> Result<usize, ProgramError> {
    let len = length as usize;
    if len > capacity {
        Err(ProgramError::InvalidAccountData)
    } else {
        Ok(len)
    }
}

#[inline(always)]
pub fn queue_views_checked(
    data: &[u8],
) -> Result<(&TransferQueueHeader, &[QueuedTransfer]), ProgramError> {
    let (header, items) = queue_views(data)?;
    if header.version != TRANSFER_QUEUE_VERSION {
        return Err(ProgramError::InvalidAccountData);
    }
    checked_active_len(header.length, items.len())?;
    Ok((header, items))
}

#[inline(always)]
pub fn queue_views_mut_checked(
    data: &mut [u8],
) -> Result<(&mut TransferQueueHeader, &mut [QueuedTransfer]), ProgramError> {
    let (header, items) = queue_views_mut(data)?;
    if header.version != TRANSFER_QUEUE_VERSION {
        return Err(ProgramError::InvalidAccountData);
    }
    checked_active_len(header.length, items.len())?;
    Ok((header, items))
}

#[inline(always)]
pub fn queue_len_and_bump_for_mint_with_capacity(
    data: &[u8],
    expected_mint: &Address,
    required_slots: usize,
) -> Result<(usize, u8), ProgramError> {
    let (header, items) = queue_views_checked(data)?;
    if header.mint != *expected_mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let queue_len = header.length as usize;
    let available_slots = items.len() - queue_len;
    if available_slots < required_slots {
        return Err(ProgramError::AccountDataTooSmall);
    }

    Ok((queue_len, header.bump))
}

#[inline(always)]
fn higher_priority(a: &QueuedTransfer, b: &QueuedTransfer) -> bool {
    if a.ready_at != b.ready_at {
        return a.ready_at < b.ready_at;
    }
    if a.inserted_at != b.inserted_at {
        return a.inserted_at < b.inserted_at;
    }
    if a.amount != b.amount {
        return a.amount < b.amount;
    }
    if a.destination_owner.as_ref() != b.destination_owner.as_ref() {
        return a.destination_owner.as_ref() < b.destination_owner.as_ref();
    }
    if a.source.as_ref() != b.source.as_ref() {
        return a.source.as_ref() < b.source.as_ref();
    }

    a.task_id < b.task_id
}

// The queue items are stored as an array-backed binary min-heap.
// Index 0 is the next transfer to execute; for node i, children are at
// 2*i+1 and 2*i+2. `heap_push` bubbles a new item up, and `heap_pop`
// moves the last item to the root and sifts it down.
#[inline(always)]
fn parent(i: usize) -> usize {
    (i - 1) / 2
}

#[inline(always)]
fn left(i: usize) -> usize {
    2 * i + 1
}

#[inline(always)]
fn right(i: usize) -> usize {
    2 * i + 2
}

fn heap_push(
    items: &mut [QueuedTransfer],
    length: &mut u32,
    transfer: QueuedTransfer,
) -> Result<(), ProgramError> {
    let len = checked_active_len(*length, items.len())?;
    if len >= items.len() {
        return Err(ProgramError::AccountDataTooSmall);
    }

    items[len] = transfer;
    *length = (len + 1) as u32;

    let mut index = len;
    while index > 0 {
        let parent_index = parent(index);
        if !higher_priority(&items[index], &items[parent_index]) {
            break;
        }
        items.swap(index, parent_index);
        index = parent_index;
    }

    Ok(())
}

fn heap_peek(items: &[QueuedTransfer], length: u32) -> Option<QueuedTransfer> {
    let len = checked_active_len(length, items.len()).ok()?;
    if len == 0 {
        None
    } else {
        Some(items[0])
    }
}

fn heap_pop(items: &mut [QueuedTransfer], length: &mut u32) -> Option<QueuedTransfer> {
    let len = checked_active_len(*length, items.len()).ok()?;
    if len == 0 {
        return None;
    }

    let popped = items[0];
    let last_index = len - 1;
    if last_index > 0 {
        items[0] = items[last_index];
    }
    items[last_index] = QueuedTransfer::zeroed();
    *length = last_index as u32;

    let mut index = 0usize;
    loop {
        let left_index = left(index);
        if left_index >= last_index {
            break;
        }

        let right_index = right(index);
        let mut best_index = left_index;
        if right_index < last_index && higher_priority(&items[right_index], &items[left_index]) {
            best_index = right_index;
        }

        if !higher_priority(&items[best_index], &items[index]) {
            break;
        }

        items.swap(index, best_index);
        index = best_index;
    }

    Some(popped)
}

/// Push one transfer into the queue account.
pub fn queue_push_from_data(
    data: &mut [u8],
    mut transfer: QueuedTransfer,
) -> Result<(), ProgramError> {
    let (header, items) = queue_views_mut_checked(data)?;
    if header.length as usize >= items.len() {
        return Err(ProgramError::AccountDataTooSmall);
    }

    let task_id = if header.next_task_id == 0 {
        1
    } else {
        header.next_task_id
    };
    transfer.task_id = task_id;

    heap_push(items, &mut header.length, transfer)?;
    header.next_task_id = task_id
        .checked_add(1)
        .ok_or(ProgramError::InvalidArgument)?;
    Ok(())
}

/// Peek the next transfer from the queue account.
pub fn queue_peek_from_data(data: &[u8]) -> Result<Option<QueuedTransfer>, ProgramError> {
    let (header, items) = queue_views_checked(data)?;
    Ok(heap_peek(items, header.length))
}

/// Pop the next transfer from the queue account.
pub fn queue_pop_from_data(data: &mut [u8]) -> Result<Option<QueuedTransfer>, ProgramError> {
    let (header, items) = queue_views_mut_checked(data)?;
    Ok(heap_pop(items, &mut header.length))
}

pub fn queue_crank_task_id_from_data(data: &[u8]) -> Result<Option<i64>, ProgramError> {
    let (header, _) = queue_views_checked(data)?;
    if header.crank_task_id == 0 {
        Ok(None)
    } else {
        Ok(Some(header.crank_task_id))
    }
}

pub fn queue_set_crank_task_id_from_data(
    data: &mut [u8],
    task_id: i64,
) -> Result<(), ProgramError> {
    let (header, _) = queue_views_mut_checked(data)?;
    header.crank_task_id = task_id;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    fn addr(byte: u8) -> Address {
        Address::new_from_array([byte; 32])
    }

    fn item(
        source: u8,
        destination: u8,
        amount: u64,
        ready_at: i64,
        inserted_at: i64,
    ) -> QueuedTransfer {
        QueuedTransfer {
            source: addr(source),
            destination_owner: addr(destination),
            amount,
            ready_at,
            inserted_at,
            task_id: 0,
            flags: 0,
            _pad0: [0; 7],
        }
    }

    #[test]
    fn queue_push_peek_pop_respects_priority() {
        let data_len = header_len() + (8 * item_len());
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(9)).unwrap();

        queue_push_from_data(data, item(2, 2, 100, 10, 5)).unwrap();
        queue_push_from_data(data, item(3, 3, 200, 10, 2)).unwrap();
        queue_push_from_data(data, item(1, 1, 50, 8, 1)).unwrap();

        let top = queue_peek_from_data(data).unwrap().unwrap();
        assert_eq!(top.ready_at, 8);
        assert_eq!(top.source, addr(1));
        assert_eq!(top.task_id, 3);

        let p1 = queue_pop_from_data(data).unwrap().unwrap();
        assert_eq!(p1.source, addr(1));
        assert_eq!(p1.task_id, 3);

        let p2 = queue_pop_from_data(data).unwrap().unwrap();
        assert_eq!(p2.source, addr(3));
        assert_eq!(p2.task_id, 2);

        let p3 = queue_pop_from_data(data).unwrap().unwrap();
        assert_eq!(p3.source, addr(2));
        assert_eq!(p3.task_id, 1);

        assert!(queue_pop_from_data(data).unwrap().is_none());
    }

    #[test]
    fn queue_amount_breaks_timestamp_ties_before_addresses() {
        let data_len = header_len() + (4 * item_len());
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(9)).unwrap();

        queue_push_from_data(data, item(1, 1, 100, 10, 5)).unwrap();
        queue_push_from_data(data, item(9, 9, 50, 10, 5)).unwrap();

        let top = queue_peek_from_data(data).unwrap().unwrap();
        assert_eq!(top.amount, 50);
        assert_eq!(top.source, addr(9));
        assert_eq!(top.destination_owner, addr(9));
    }

    #[test]
    fn queue_crank_task_id_round_trips() {
        let data_len = header_len() + item_len();
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(7)).unwrap();
        assert_eq!(queue_crank_task_id_from_data(data).unwrap(), None);

        queue_set_crank_task_id_from_data(data, 42).unwrap();
        assert_eq!(queue_crank_task_id_from_data(data).unwrap(), Some(42));
    }

    #[test]
    fn unknown_version_is_rejected() {
        let data_len = header_len() + item_len();
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        let (header, _) = queue_views_mut(data).unwrap();
        header.version = 99;

        match queue_peek_from_data(data) {
            Err(err) => assert_eq!(err, ProgramError::InvalidAccountData),
            Ok(_) => panic!("expected invalid account data"),
        }
    }
}
