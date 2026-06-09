use alloc::vec::Vec;

use wheels::variable_offset_layout;

use crate::Address;

#[variable_offset_layout(buffer_offset = 1)]
pub struct SchedulePrivateTransferArgs {
    pub shuttle_id: u32,
    pub stash_bump: u8,
    pub mint: [u8; 32],
    pub shuttle_bump: u8,
    pub shuttle_eata_bump: u8,
    pub shuttle_wallet_ata_bump: u8,
    pub buffer_bump: u8,
    pub delegation_record_bump: u8,
    pub delegation_metadata_bump: u8,
    pub global_vault_bump: u8,
    pub vault_token_bump: u8,
    pub stash_ata_bump: u8,
    pub queue_bump: u8,
    pub validator: [u8; 32],
    pub encrypted_destination: [u8; 80],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}

impl SchedulePrivateTransferArgsView<'_> {
    pub fn validator_address(&self) -> &Address {
        unsafe { &*(self.validator().as_ptr() as *const Address) }
    }

    pub fn mint_address(&self) -> &Address {
        unsafe { &*(self.mint().as_ptr() as *const Address) }
    }
}
