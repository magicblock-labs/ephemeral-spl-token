use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1)]
pub struct AmountAndSaltArgs {
    pub amount: u64,
    pub salt: [u8; 32],
}
