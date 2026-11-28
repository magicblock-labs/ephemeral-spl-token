use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1)]
pub struct InitializeShuttleArgs {
    pub shuttle_id: u32,
}
