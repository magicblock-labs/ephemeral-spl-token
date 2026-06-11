use solana_address::Address;
use solana_pubkey::pubkey;

/// TODO: can be removed once pinocchio SDK supports this
/// Callback signer PDA info
pub(crate) const CALLBACK_PROGRAM_ID: Address =
    pubkey!("CaLLback11111111111111111111111111111111111");
pub(crate) const CALLBACK_SEED: &[u8] = b"callback-executor";
const CALLBACK_SIGNER_PDA: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[CALLBACK_SEED], CALLBACK_PROGRAM_ID.as_array());
pub(crate) const CALLBACK_SIGNER: Address = Address::new_from_array(CALLBACK_SIGNER_PDA.0);
