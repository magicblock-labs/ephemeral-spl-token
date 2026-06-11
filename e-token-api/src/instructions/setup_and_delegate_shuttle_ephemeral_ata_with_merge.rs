use wheels::variable_offset_layout;

use solana_address::Address;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DepositAndDelegateShuttleArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(DepositAndDelegateShuttleArgs::DATA_LENS, [12, 44]));
