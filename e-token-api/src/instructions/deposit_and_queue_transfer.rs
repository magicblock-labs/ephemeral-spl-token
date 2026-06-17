use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DepositAndQueueTransferArgs {
    pub amount: u64,
    pub group_id: [u8; 3],
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub split: u32,
    pub flags: Option<u8>,
    pub client_ref_id: Option<u64>,
}

static_assertions::const_assert!(matches!(
    DepositAndQueueTransferArgs::DATA_LENS,
    [31, 32, 39, 40]
));

impl DepositAndQueueTransferArgsView<'_> {
    pub fn group_id_u32(&self) -> u32 {
        let id = self.group_id();
        u32::from(id[0]) | (u32::from(id[1]) << 8) | (u32::from(id[2]) << 16)
    }
}
