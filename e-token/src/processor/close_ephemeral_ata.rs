use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Any     : Owner of the ephemeral ATA.
///  1: [writable]          - PDA     : Ephemeral ATA account (PDA derived from [owner, mint]).
///  2: [writable]          - Any     : Recipient account for rent refund.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_close_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        owner_info, // force multi-line
        ephemeral_ata_info,
        recipient_info,
    ] = require_n_accounts!(accounts, 3);

    require!(
        owner_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let (mint, lamports_to_refund, bump) = {
        let ephemeral_ata =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        require_eq_keys!(
            &ephemeral_ata.owner,
            owner_info.address(),
            ProgramError::IncorrectAuthority
        );
        require!(ephemeral_ata.amount == 0, ProgramError::InvalidArgument);

        (
            ephemeral_ata.mint,
            ephemeral_ata_info.lamports(),
            ephemeral_ata.bump,
        )
    };

    let derived_pda = EphemeralAta::derive_pda(owner_info.address(), &mint, bump)?;
    require_eq_keys!(
        &derived_pda,
        ephemeral_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    require!(
        recipient_info.address() != ephemeral_ata_info.address(),
        ProgramError::InvalidArgument
    );

    let updated_recipient_lamports = recipient_info
        .lamports()
        .checked_add(lamports_to_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    recipient_info.set_lamports(updated_recipient_lamports);
    ephemeral_ata_info.set_lamports(0);
    ephemeral_ata_info.close()?;

    Ok(())
}
