use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

/// Undelegate a Shuttle Ephemeral ATA by calling into the delegation program
/// helper that schedules a commit and performs undelegation.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Payer
/// 1. [writable] User ATA account (SPL ATA for [payer, mint])
/// 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
/// 3. []         Shuttle EATA account (PDA [shuttle_metadata, mint])
/// 4. [writable] Magic context account (as required by the delegation program)
/// 5. []         Delegation program ID (aka magic program)
pub fn process_undelegate_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer, ata_info, shuttle_info, shuttle_eata_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
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

    let (mint, bump) = {
        let shuttle_eata =
            unsafe { load_unchecked::<EphemeralAta>(shuttle_eata_info.borrow_unchecked())? };
        if shuttle_eata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_eata.mint.clone();
        (mint, shuttle_eata.bump)
    };

    let derived_shuttle_eata =
        EphemeralAta::create_address(&shuttle_info.address(), &mint, &[bump])?;
    if derived_shuttle_eata != *shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    {
        let token_data = unsafe { ata_info.borrow_unchecked() };
        if token_data.len() < TokenAccount::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let token_acc = unsafe { TokenAccount::from_bytes_unchecked(token_data) };
        if !token_acc.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if token_acc.mint() != &mint || token_acc.owner() != payer.address() {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        &[ata_info.clone()],
        magic_context,
        magic_program,
    )
}
