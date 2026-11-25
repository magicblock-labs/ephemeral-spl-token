use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = any)]
pub struct AsyncLamportsTransferArgs {
    pub amount: u64,
    pub salt: [u8; 32],
}
