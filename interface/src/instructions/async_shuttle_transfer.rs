use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = any, option = implicit)]
pub struct AsyncShuttleTransferArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(AsyncShuttleTransferArgs::DATA_LENS, [12, 44]));
