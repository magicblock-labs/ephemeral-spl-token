use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct EnsureStealthPoolDelegatedArgs {
    pub handle: [u8; 256],
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(EnsureStealthPoolDelegatedArgs::DATA_LENS, [256, 288]));
