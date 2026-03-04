use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

/// Commit and undelegate the shuttle EATA.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Payer (must match shuttle.payer)
/// 1. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
/// 2. [writable] Shuttle EATA account (PDA [shuttle_metadata, mint])
/// 3. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
/// 4. [writable] Magic context account
/// 5. []         Magic program
pub fn process_undelegate_and_close_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if shuttle_info.is_writable() {
        return Err(ProgramError::InvalidArgument);
    }

    unsafe {
        if shuttle_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
    if !shuttle.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle.payer != *payer.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let mint = {
        let shuttle_ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(shuttle_ephemeral_ata_info.borrow_unchecked())? };
        if shuttle_ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_ephemeral_ata.mint.clone();
        mint
    };

    let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_shuttle_ephemeral_ata != *shuttle_ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    {
        let shuttle_wallet_data = unsafe { shuttle_wallet_ata_info.borrow_unchecked() };
        if shuttle_wallet_data.len() < TokenAccount::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let shuttle_wallet = unsafe { TokenAccount::from_bytes_unchecked(shuttle_wallet_data) };
        if !shuttle_wallet.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if shuttle_wallet.owner() != shuttle_info.address() || shuttle_wallet.mint() != &mint {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        &[shuttle_wallet_ata_info.clone()],
        magic_context,
        magic_program,
    )
}
