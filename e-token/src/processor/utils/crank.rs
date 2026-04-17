use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;

/// TODO: can be removed once pinnochio SDK supports this
/// Seed for crank signer PDA that is used currently in magic-program
pub const CRANK_SEED: &[u8] = b"crank-executor";

const CRANK_SIGNER_PDA: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[CRANK_SEED], MAGIC_PROGRAM_ID.as_array());

/// Signer that authorizes crank tick
pub const CRANK_SIGNER: ephemeral_spl_api::Address =
    ephemeral_spl_api::Address::new_from_array(CRANK_SIGNER_PDA.0);
