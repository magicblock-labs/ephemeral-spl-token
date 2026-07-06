use solana_address::Address;

/// Wrapped SOL (native) mint address `So11111111111111111111111111111111111111112`.
pub(crate) const NATIVE_MINT: Address = Address::new_from_array([
    6, 155, 136, 87, 254, 171, 129, 132, 251, 104, 127, 99, 70, 24, 192, 53, 218, 196, 57, 220, 26, 235, 59, 85, 152,
    160, 240, 0, 0, 0, 0, 1,
]);

/// PDA that owns the ephemeral scratch wrapped-SOL token account used to unwrap native
/// deliveries. The account is created, funded, and closed within a single settlement
/// instruction, so a single shared PDA is sufficient (transactions writing the same
/// scratch account are serialized by the runtime).
pub(crate) const UNWRAP_PDA_SEED: &[u8] = b"unwrap";
const UNWRAP_PDA_AND_BUMP: ([u8; 32], u8) =
    const_crypto::ed25519::derive_program_address(&[UNWRAP_PDA_SEED], crate::ID.as_array());
pub(crate) const UNWRAP_PDA: Address = Address::new_from_array(UNWRAP_PDA_AND_BUMP.0);
pub(crate) const UNWRAP_PDA_BUMP: u8 = UNWRAP_PDA_AND_BUMP.1;
