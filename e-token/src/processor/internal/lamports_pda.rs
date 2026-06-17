use pinocchio::Address;

pub(crate) const LAMPORTS_PDA_SEED: &[u8] = b"lamports";

pub(crate) fn derive_lamports_pda(
    payer: &Address,
    destination: &Address,
    salt: &[u8; 32],
) -> (Address, u8) {
    Address::find_program_address(
        &[
            LAMPORTS_PDA_SEED,
            payer.as_ref(),
            destination.as_ref(),
            salt.as_ref(),
        ],
        &crate::ID,
    )
}
