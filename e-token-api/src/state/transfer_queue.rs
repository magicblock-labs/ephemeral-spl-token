use bytemuck::{Pod, Zeroable};
use pinocchio::{error::ProgramError, Address};

/// Current queue version that stores ready timestamps in milliseconds and client reference ids.
/// Bump this value only when the on-chain layout changes or queue semantics require it.
pub const TRANSFER_QUEUE_VERSION: u8 = 1;
/// PDA seed prefix for transfer queues.
pub const QUEUE_SEED: &[u8] = b"queue";
pub const QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA: u8 = 1 << 0;
pub const MAX_GROUP_ID: u32 = 0x00FF_FFFF;

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
    pub _pad1: [u8; 8],
    pub next_task_id: u32,
    pub crank_task_id: i64,
    pub validator: Address,
}

/// One queued transfer entry.
///
/// `ready_at` is stored in milliseconds since unix epoch. `client_ref_id` is
/// an opaque caller-provided correlation id, or zero when omitted.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, PartialEq, Eq)]
pub struct QueuedTransfer {
    pub source: Address,
    pub destination_owner: Address,
    pub amount: u64,
    pub ready_at: i64,
    pub client_ref_id: u64,
    pub task_id: u32,
    pub flags: u8,
    pub _pad0: [u8; 3],
}

impl QueuedTransfer {
    #[inline(always)]
    pub fn should_create_destination_ata_idempotent(&self) -> bool {
        self.flags & QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA != 0
    }

    #[inline(always)]
    pub fn group_id(&self) -> u32 {
        u32::from(self._pad0[0])
            | (u32::from(self._pad0[1]) << 8)
            | (u32::from(self._pad0[2]) << 16)
    }

    #[inline(always)]
    pub fn set_group_id(&mut self, group_id: u32) -> Result<(), ProgramError> {
        if group_id == 0 || group_id > MAX_GROUP_ID {
            return Err(ProgramError::InvalidArgument);
        }

        self._pad0[0] = group_id as u8;
        self._pad0[1] = (group_id >> 8) as u8;
        self._pad0[2] = (group_id >> 16) as u8;
        Ok(())
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
pub fn init_queue(
    data: &mut [u8],
    bump: u8,
    mint: Address,
    validator: Address,
) -> Result<(), ProgramError> {
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
            header.validator = validator;
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
fn stored_next_group_id(header: &TransferQueueHeader) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&header._pad1[..4]);
    u32::from_le_bytes(bytes)
}

#[inline(always)]
fn set_stored_next_group_id(header: &mut TransferQueueHeader, group_id: u32) {
    header._pad1[..4].copy_from_slice(&group_id.to_le_bytes());
}

#[inline(always)]
fn normalize_group_id(group_id: u32) -> u32 {
    if group_id == 0 || group_id > MAX_GROUP_ID {
        1
    } else {
        group_id
    }
}

#[inline(always)]
fn next_group_id(group_id: u32) -> u32 {
    if group_id >= MAX_GROUP_ID {
        1
    } else {
        group_id + 1
    }
}

#[inline(always)]
fn group_id_in_use(
    items: &[QueuedTransfer],
    length: u32,
    group_id: u32,
) -> Result<bool, ProgramError> {
    let len = checked_active_len(length, items.len())?;
    Ok(items[..len].iter().any(|item| item.group_id() == group_id))
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
pub fn queue_len_for_mint_with_capacity(
    data: &[u8],
    expected_mint: &Address,
    required_slots: usize,
) -> Result<(usize, Address), ProgramError> {
    let (header, items) = queue_views_checked(data)?;
    if header.mint != *expected_mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let queue_len = header.length as usize;
    let available_slots = items.len() - queue_len;
    if available_slots < required_slots {
        return Err(ProgramError::AccountDataTooSmall);
    }

    Ok((queue_len, header.validator))
}

pub fn queue_allocate_group_id_from_data(data: &mut [u8]) -> Result<u32, ProgramError> {
    let (header, items) = queue_views_mut_checked(data)?;
    let mut candidate = normalize_group_id(stored_next_group_id(header));

    for _ in 0..MAX_GROUP_ID {
        if !group_id_in_use(items, header.length, candidate)? {
            set_stored_next_group_id(header, next_group_id(candidate));
            return Ok(candidate);
        }
        candidate = next_group_id(candidate);
    }

    Err(ProgramError::InvalidAccountData)
}

#[inline(always)]
fn higher_priority(a: &QueuedTransfer, b: &QueuedTransfer) -> bool {
    if a.ready_at != b.ready_at {
        return a.ready_at < b.ready_at;
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

#[inline(always)]
fn effective_next_task_id(length: u32, next_task_id: u32) -> u32 {
    let next_task_id = if length == 0 && next_task_id > (u32::MAX / 2) {
        0
    } else {
        next_task_id
    };

    if next_task_id == 0 { 1 } else { next_task_id }
}

#[inline(always)]
fn maybe_reset_next_task_id(header: &mut TransferQueueHeader) {
    if header.length == 0 && header.next_task_id > (u32::MAX / 2) {
        header.next_task_id = 0;
    }
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
    maybe_reset_next_task_id(header);
    if header.length as usize >= items.len() {
        return Err(ProgramError::AccountDataTooSmall);
    }

    let task_id = effective_next_task_id(header.length, header.next_task_id);
    transfer.task_id = task_id;

    heap_push(items, &mut header.length, transfer)?;
    header.next_task_id = task_id
        .checked_add(1)
        .ok_or(ProgramError::InvalidArgument)?;
    Ok(())
}

/// Peek the effective next task id that would be assigned by `queue_push_from_data`.
pub fn queue_peek_next_task_id_from_data(data: &[u8]) -> Result<u32, ProgramError> {
    let (header, _) = queue_views_checked(data)?;
    Ok(effective_next_task_id(header.length, header.next_task_id))
}

/// Peek the next transfer from the queue account.
pub fn queue_peek_from_data(data: &[u8]) -> Result<Option<QueuedTransfer>, ProgramError> {
    let (header, items) = queue_views_checked(data)?;
    Ok(heap_peek(items, header.length))
}

/// Pop the next transfer from the queue account.
pub fn queue_pop_from_data(data: &mut [u8]) -> Result<Option<QueuedTransfer>, ProgramError> {
    let (header, items) = queue_views_mut_checked(data)?;
    let popped = heap_pop(items, &mut header.length);
    maybe_reset_next_task_id(header);
    Ok(popped)
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
        client_ref_id: u64,
    ) -> QueuedTransfer {
        QueuedTransfer {
            source: addr(source),
            destination_owner: addr(destination),
            amount,
            ready_at,
            client_ref_id,
            task_id: 0,
            flags: 0,
            _pad0: [0; 3],
        }
    }

    #[test]
    fn queue_push_peek_pop_respects_priority() {
        let data_len = header_len() + (8 * item_len());
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(9), addr(10)).unwrap();

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
        assert_eq!(p2.source, addr(2));
        assert_eq!(p2.task_id, 1);

        let p3 = queue_pop_from_data(data).unwrap().unwrap();
        assert_eq!(p3.source, addr(3));
        assert_eq!(p3.task_id, 2);

        assert!(queue_pop_from_data(data).unwrap().is_none());
    }

    #[test]
    fn queue_amount_breaks_timestamp_ties_before_addresses() {
        let data_len = header_len() + (4 * item_len());
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(9), addr(10)).unwrap();

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

        init_queue(data, 1, addr(7), addr(10)).unwrap();
        assert_eq!(queue_crank_task_id_from_data(data).unwrap(), None);

        queue_set_crank_task_id_from_data(data, 42).unwrap();
        assert_eq!(queue_crank_task_id_from_data(data).unwrap(), Some(42));
    }

    #[test]
    fn queued_transfer_group_id_round_trips() {
        let mut transfer = item(1, 2, 10, 3, 2);
        transfer.set_group_id(MAX_GROUP_ID).unwrap();
        assert_eq!(transfer.group_id(), MAX_GROUP_ID);
        assert_eq!(transfer.set_group_id(0), Err(ProgramError::InvalidArgument));
        assert_eq!(
            transfer.set_group_id(MAX_GROUP_ID + 1),
            Err(ProgramError::InvalidArgument)
        );
    }

    #[test]
    fn queue_allocate_group_id_skips_live_ids_and_wraps() {
        let data_len = header_len() + (3 * item_len());
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(7), addr(10)).unwrap();
        {
            let (header, items) = queue_views_mut_checked(data).unwrap();
            header.length = 2;
            set_stored_next_group_id(header, MAX_GROUP_ID);
            items[0] = item(1, 2, 10, 3, 2);
            items[0].set_group_id(MAX_GROUP_ID).unwrap();
            items[1] = item(3, 4, 20, 4, 3);
            items[1].set_group_id(1).unwrap();
        }

        let group_id = queue_allocate_group_id_from_data(data).unwrap();
        assert_eq!(group_id, 2);

        let (header, _) = queue_views_checked(data).unwrap();
        assert_eq!(stored_next_group_id(header), 3);
    }

    #[test]
    fn queue_push_resets_next_task_id_when_empty_and_past_half_max() {
        let data_len = header_len() + item_len();
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(7), addr(10)).unwrap();
        let (header, _) = queue_views_mut_checked(data).unwrap();
        header.next_task_id = (u32::MAX / 2) + 1;

        queue_push_from_data(data, item(1, 2, 10, 3, 2)).unwrap();

        let (header, items) = queue_views_checked(data).unwrap();
        assert_eq!(header.next_task_id, 2);
        assert_eq!(items[0].task_id, 1);
    }

    #[test]
    fn queue_peek_next_task_id_applies_reset_rules() {
        let data_len = header_len() + item_len();
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(7), addr(10)).unwrap();
        let (header, _) = queue_views_mut_checked(data).unwrap();
        header.next_task_id = (u32::MAX / 2) + 1;

        assert_eq!(queue_peek_next_task_id_from_data(data).unwrap(), 1);
    }

    #[test]
    fn queue_pop_resets_next_task_id_when_empty_and_past_half_max() {
        let data_len = header_len() + item_len();
        let words = data_len.div_ceil(8);
        let mut aligned = std::vec![0u64; words];
        let data = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut aligned)[..data_len];

        init_queue(data, 1, addr(7), addr(10)).unwrap();
        {
            let (header, items) = queue_views_mut_checked(data).unwrap();
            header.length = 1;
            header.next_task_id = (u32::MAX / 2) + 1;
            items[0] = item(1, 2, 10, 3, 2);
        }

        let popped = queue_pop_from_data(data).unwrap().unwrap();
        assert_eq!(popped.amount, 10);

        let (header, items) = queue_views_checked(data).unwrap();
        assert_eq!(header.length, 0);
        assert_eq!(header.next_task_id, 0);
        assert!(items[0] == QueuedTransfer::zeroed());
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
