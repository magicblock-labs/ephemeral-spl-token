use solana_address::Address;
use wheels::variable_offset_layout;

pub(crate) const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

pub(crate) fn derive_group_receipt_id(queue_address: &Address, source: &Address, group_id: u32) -> (Address, u8) {
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

#[variable_offset_layout(buffer_offset = any)]
pub(crate) struct TransferCallbackArgs {
    /// Amount was transferred in action
    pub amount: u64,
    /// Group ID of a transfer
    pub group_id: u32,
    // Flags
    pub flag: u8,
}
