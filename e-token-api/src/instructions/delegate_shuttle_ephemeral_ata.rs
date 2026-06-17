use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DelegateShuttleArgs {
    pub validator: Option<[u8; 32]>,
}

static_assertions::const_assert!(matches!(DelegateShuttleArgs::DATA_LENS, [0, 32]));
