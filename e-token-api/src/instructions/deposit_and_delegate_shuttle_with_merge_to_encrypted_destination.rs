use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub encrypted_destination_owner: [u8; 80],
    pub encrypted_destination_ata: [u8; 80],
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(
    DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs::DATA_LENS,
    [172, 204]
));
