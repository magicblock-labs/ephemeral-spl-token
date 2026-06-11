use wheels::variable_offset_layout;

use solana_address::Address;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DelegateArgs {
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(DelegateArgs::DATA_LENS, [0, 32]));
