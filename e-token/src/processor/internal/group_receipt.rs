use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use ephemeral_spl_api::Address;

pub(crate) const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

pub(crate) fn derive_group_receipt_id(
    queue_address: &Address,
    source: &Address,
    group_id: u32,
) -> (Address, u8) {
    Address::find_program_address(
        &[
            GROUP_RECEIPT_SEED,
            queue_address.as_ref(),
            source.as_ref(),
            &group_id.to_le_bytes(),
        ],
        &crate::ID,
    )
}

// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
pub(crate) struct TransferCallbackArgs {
    /// Amount was transferred in action
    pub amount: u64,
    /// Group ID of a transfer
    pub group_id: u32,
    // Flags
    pub flag: u8,
}
