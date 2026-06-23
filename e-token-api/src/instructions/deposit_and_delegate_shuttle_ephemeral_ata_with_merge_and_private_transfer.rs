use alloc::vec::Vec;

use solana_address::Address;
use wheels::variable_offset_layout;

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

#[variable_offset_layout(buffer_offset = unaligned)]
#[derive(Clone, Copy)]
pub struct CloseStashArgs {
    pub user: Address,
    pub stash_bump: u8,
}

static_assertions::const_assert_eq!(CloseStashArgs::DATA_LEN, 33);
