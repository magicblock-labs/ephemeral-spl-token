use pinocchio::{error::ProgramError, Address};

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

#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_amount_and_salt(
    instruction_data: &[u8],
) -> Result<(u64, [u8; 32]), ProgramError> {
    if instruction_data.len() != 40 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut amount = [0u8; 8];
    amount.copy_from_slice(&instruction_data[..8]);

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&instruction_data[8..40]);

    Ok((u64::from_le_bytes(amount), salt))
}
