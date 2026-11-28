use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = any)]
pub struct InitializeShuttleArgs {
    pub shuttle_id: u32,
}
