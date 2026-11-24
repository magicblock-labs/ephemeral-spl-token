use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = any)]
pub struct DepositArgs {
    pub amount: u64,
}
