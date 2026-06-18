use alloc::vec::Vec;

use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1)]
pub struct ExecuteScheduledPrivateTransferArgs {
    pub user: Address,
    pub stash_bump: u8,
    pub shuttle_id: u32,
    pub validator: Address,
    pub encrypted_destination: [u8; 80],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}
