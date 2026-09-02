use solana_address::Address;
use wheels::variable_offset_layout;

#[variable_offset_layout(buffer_offset = 1, option = implicit)]
pub struct UndelegateArgs {
    /// Validator identity the eATA is delegated to. Required when the magic
    /// fee vault account is passed, so the program can verify the vault PDA.
    pub validator: Option<Address>,
}

static_assertions::const_assert!(matches!(UndelegateArgs::DATA_LENS, [0, 32]));
