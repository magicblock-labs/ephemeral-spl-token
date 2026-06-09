use alloc::vec;
use alloc::vec::Vec;

use data_layout::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DelegateArgs {
    pub validator: Option<[u8; 32]>,
}

static_assertions::const_assert!(matches!(DelegateArgs::DATA_LENS, [0, 32]));
