use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::ephemeral_ata::initialize_ephemeral_ata_with_sponsor;

#[inline(always)]
pub fn process_initialize_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [user, mint])
    // 1. []         Payer (funding account)
    // 2. []         User  (seed)
    // 3. []         Mint  (seed)

    let [ephemeral_ata_info, payer_info, user_info, mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    initialize_ephemeral_ata_with_sponsor(
        ephemeral_ata_info,
        payer_info,
        None,
        user_info,
        mint_info,
    )
}
