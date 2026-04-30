use solana_pubkey::pubkey;

/// TODO: can be removed once pinocchio SDK supports this
/// Crank signer PDA info
pub const CRANK_PROGRAM_ID: ephemeral_spl_api::Address =
    pubkey!("Crank11111111111111111111111111111111111111");
pub const CRANK_SEED: &[u8] = b"crank-executor";

const CRANK_SIGNER_PDA: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[CRANK_SEED], CRANK_PROGRAM_ID.as_array());

pub const CRANK_SIGNER: ephemeral_spl_api::Address =
    ephemeral_spl_api::Address::new_from_array(CRANK_SIGNER_PDA.0);

/// Callback signer PDA info
pub const CALLBACK_PROGRAM_ID: ephemeral_spl_api::Address =
    pubkey!("CaLLback11111111111111111111111111111111111");
pub const CALLBACK_SEED: &[u8] = b"callback-executor";
const CALLBACK_SIGNER_PDA: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[CALLBACK_SEED], CALLBACK_PROGRAM_ID.as_array());
pub const CALLBACK_SIGNER: ephemeral_spl_api::Address =
    ephemeral_spl_api::Address::new_from_array(CALLBACK_SIGNER_PDA.0);
pub const CALLBACK_SIGNER_BUMP: u8 = CALLBACK_SIGNER_PDA.1;
