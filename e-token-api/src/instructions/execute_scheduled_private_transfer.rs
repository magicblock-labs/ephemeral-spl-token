use alloc::vec;
use alloc::vec::Vec;

use data_layout::variable_offset_layout;

use crate::Address;

#[variable_offset_layout(buffer_offset = 1)]
pub struct ExecuteScheduledPrivateTransferArgs {
    pub user: [u8; 32],
    pub stash_bump: u8,
    pub shuttle_id: u32,
    pub validator: [u8; 32],
    pub encrypted_destination: [u8; 80],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}

impl ExecuteScheduledPrivateTransferArgsView<'_> {
    pub fn user_address(&self) -> &Address {
        unsafe { &*(self.user().as_ptr() as *const Address) }
    }
}
