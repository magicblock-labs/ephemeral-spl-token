use alloc::vec;
use alloc::vec::Vec;

use data_layout::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DepositAndDelegateShuttleArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<[u8; 32]>,
}

static_assertions::const_assert!(matches!(DepositAndDelegateShuttleArgs::DATA_LENS, [12, 44]));
