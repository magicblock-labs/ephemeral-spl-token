use {
    pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult},
    ephemeral_spl_api::EphemeralAta,
};

#[inline(always)]
pub fn process_initialize_ephemeral_ata(accounts: &[AccountInfo], _instruction_data: &[u8]) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [payer, mint])
    // 1. []         Payer (seed)
    // 2. []         Mint  (seed)
    let [ephemeral_ata_info, _payer_info, _mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Ensure account data can hold the EphemeralAta structure
    if ephemeral_ata_info.data_len() < EphemeralAta::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    // Initialize with balance = 0
    // SAFETY: single mutable borrow of the account data slice.
    let data = unsafe { ephemeral_ata_info.borrow_mut_data_unchecked() };
    // Write zero to the first 8 bytes representing balance (u64 LE)
    for b in &mut data[..EphemeralAta::LEN] {
        *b = 0;
    }

    Ok(())
}
