use wheels::variable_offset_layout;

use solana_address::Address;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct EnsureStealthPoolDelegatedArgs {
    pub handle_hash: [u8; 32],
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(
    EnsureStealthPoolDelegatedArgs::DATA_LENS,
    [32, 64]
));
