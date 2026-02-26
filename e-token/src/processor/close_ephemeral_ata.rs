use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked, Initializable};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

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

    if !owner_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    unsafe {
        if ephemeral_ata_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let (mint, lamports_to_refund, bump) = {
        let ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked())? };

        if !ephemeral_ata.is_initialized() {
            return Err(ProgramError::InvalidAccountData);
        }

        if ephemeral_ata.owner != *owner_info.address() {
            return Err(ProgramError::IncorrectAuthority);
        }

        if ephemeral_ata.amount != 0 {
            return Err(ProgramError::InvalidArgument);
        }

        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        (mint, ephemeral_ata_info.lamports(), ephemeral_ata.bump)
    };

    let derived_pda = EphemeralAta::create_address(&owner_info.address(), &mint, &[bump])?;
    if &derived_pda != ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    };

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
