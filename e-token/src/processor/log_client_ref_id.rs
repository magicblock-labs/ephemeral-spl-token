use pinocchio::{error::ProgramError, AccountView, ProgramResult};

#[inline(always)]
pub fn process_log_client_ref_id(
    _accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let client_ref_id_bytes: [u8; 8] = instruction_data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let client_ref_id = u64::from_le_bytes(client_ref_id_bytes);

    pinocchio_log::log!("client_ref_id: {}", client_ref_id);

    Ok(())
}
