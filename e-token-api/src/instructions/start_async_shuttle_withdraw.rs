use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct StartAsyncShuttleWithdrawArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(StartAsyncShuttleWithdrawArgs::DATA_LENS, [12, 44]));
