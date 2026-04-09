use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::assert_owner;
use crate::processor::{
    shuttle_close_schedule::{parse_escrow_index, schedule_shuttle_close_after_undelegate},
    utils::{get_associated_token_address, validate_token_account},
};

/// Commit and undelegate shuttle wallet ATA, then schedule a post-undelegate
/// action that closes shuttle wallet ATA and shuttle EATA if amount == 0, then
/// closes shuttle metadata account and sends rent to the stored reimbursement recipient.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Executor payer
/// 1. [writable] Rent reimbursement account (must match shuttle.payer)
/// 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
/// 3. []         Shuttle EATA account
/// 4. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
/// 5. [writable] Refund token ATA
/// 6. []         Token program account
/// 7. [writable] Magic context account
/// 8. []         Magic program
pub fn process_undelegate_and_close_shuttle_to_owner(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;

    let [executor, rent_reimbursement, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, refund_token_info, token_program_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !executor.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    assert_owner!(shuttle_info, &crate::ID);

    let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
    if shuttle.payer != *rent_reimbursement.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let mint = {
        let shuttle_ephemeral_ata = load_initialized::<EphemeralAta>(unsafe {
            shuttle_ephemeral_ata_info.borrow_unchecked()
        })?;
        if shuttle_ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_ephemeral_ata.mint.clone();
        mint
    };

    let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref()],
        &crate::ID,
    );
    if derived_shuttle_ephemeral_ata != *shuttle_ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let expected_shuttle_wallet_ata =
        get_associated_token_address(shuttle_info.address(), &mint, token_program_info.address());
    if expected_shuttle_wallet_ata != *shuttle_wallet_ata_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    validate_token_account(
        shuttle_wallet_ata_info,
        &mint,
        Some(shuttle_info.address()),
        Some(token_program_info.address()),
    )?;
    validate_token_account(
        refund_token_info,
        &mint,
        Some(&shuttle.owner),
        Some(token_program_info.address()),
    )?;

    schedule_shuttle_close_after_undelegate(
        executor,
        rent_reimbursement,
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
        refund_token_info,
        &mint,
        token_program_info,
        magic_context,
        magic_program,
        escrow_index,
    )
}
