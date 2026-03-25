use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::{assert_owner, assert_signer};

#[inline(always)]
pub fn process_close_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Owner of the ephemeral ATA
    // 1. [writable] Ephemeral ATA account (PDA [owner, mint])
    // 2. [writable] Recipient account for rent refund
    let [owner_info, ephemeral_ata_info, recipient_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(owner_info);
    assert_owner!(
        ephemeral_ata_info,
        &ephemeral_spl_api::program::id_address()
    );

    let (mint, lamports_to_refund) = {
        let ephemeral_ata =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        if ephemeral_ata.owner != *owner_info.address() {
            return Err(ProgramError::IncorrectAuthority);
        }
        if ephemeral_ata.amount != 0 {
            return Err(ProgramError::InvalidArgument);
        }

        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        (mint, ephemeral_ata_info.lamports())
    };

    let (derived_pda, _) = ephemeral_spl_api::Address::find_program_address(
        &[owner_info.address().as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_pda != *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if *recipient_info.address() == *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidArgument);
    }

    let updated_recipient_lamports = recipient_info
        .lamports()
        .checked_add(lamports_to_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    recipient_info.set_lamports(updated_recipient_lamports);
    ephemeral_ata_info.set_lamports(0);
    ephemeral_ata_info.close()?;

    Ok(())
}
