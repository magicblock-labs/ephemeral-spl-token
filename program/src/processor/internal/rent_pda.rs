use solana_address::Address;

pub(crate) const RENT_PDA_SEED: &[u8] = b"rent";
const RENT_PDA_AND_BUMP: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[RENT_PDA_SEED], crate::ID.as_array());
pub(crate) const RENT_PDA: Address = Address::new_from_array(RENT_PDA_AND_BUMP.0);
pub(crate) const RENT_PDA_BUMP: u8 = RENT_PDA_AND_BUMP.1;
