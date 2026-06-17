use alloc::vec::Vec;

use wheels::variable_offset_layout;

use solana_address::Address;

#[variable_offset_layout(buffer_offset = 1)]
pub struct DepositAndDelegateShuttleWithPrivateTransferArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub exact_out: bool,
    pub encrypted_destination: [u8; 80],
    pub validator: Option<Address>,
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}
