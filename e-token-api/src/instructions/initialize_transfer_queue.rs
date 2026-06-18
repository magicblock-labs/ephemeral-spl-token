use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct InitializeTransferQueueArgs {
    pub requested_items: Option<u32>,
}

static_assertions::const_assert!(matches!(InitializeTransferQueueArgs::DATA_LENS, [0, 4]));
