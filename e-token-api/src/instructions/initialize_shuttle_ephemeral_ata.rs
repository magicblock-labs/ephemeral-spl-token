use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1)]
pub struct InitializeShuttleEphemeralAtaArgs {
    pub shuttle_id: u32,
}
