use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct ExecuteQueuedTransferArgs {
    pub escrow_index: u8,
    pub amount: u64,
    pub flags: u8,
    pub client_ref_id: Option<u64>,
}

static_assertions::const_assert!(matches!(ExecuteQueuedTransferArgs::DATA_LENS, [10, 18]));

impl ExecuteQueuedTransferArgsView<'_> {
    pub fn should_create_destination_ata_idempotent(&self) -> bool {
        self.flags() & crate::state::transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA != 0
    }
}
