use alloc::vec::Vec;

use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1)]
pub struct UpdateStealthPoolArgs {
    pub handle: [u8; 256],
    // TODO (snawaz): support enum based flags in data-layout
    pub flags: u8,
    #[flexible = 1]
    pub destinations: Vec<Address>,
}

static_assertions::const_assert!(UpdateStealthPoolArgs::DATA_LEN_RANGE.0 == 258);
static_assertions::const_assert!(UpdateStealthPoolArgs::DATA_LEN_RANGE.1 == 8418);
