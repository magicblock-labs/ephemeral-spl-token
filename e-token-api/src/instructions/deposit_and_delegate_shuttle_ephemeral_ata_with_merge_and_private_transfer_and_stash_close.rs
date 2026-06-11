use alloc::vec::Vec;

use wheels::variable_offset_layout;

use crate::Address;

#[variable_offset_layout(buffer_offset = 1)]
pub struct DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub exact_out: bool,
    pub encrypted_destination: [u8; 80],
    pub validator: Option<Address>,
    pub stash_close_seeds: [u8; 33],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}
