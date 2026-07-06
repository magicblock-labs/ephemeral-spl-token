use ephemeral_spl_api::{
    require,
    state::{load_initialized, shuttle_ephemeral_ata::ShuttleMetadata},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::{close_shuttle_ata_intent::settle_and_close_shuttle_accounts, internal::validate_token_account};

///
/// Executes on: BASE only.
///
/// Permissionless recovery for a shuttle whose EATA is already undelegated.
/// The caller pays the transaction fee, but settlement can only go to a token
/// account owned by `ShuttleMetadata.owner`.
///
/// Accounts:
///
///  0: [writable]          - Any     : Shuttle rent reimbursement account (must equal `ShuttleMetadata.payer`).
///  1: [writable]          - PDA     : Shuttle metadata account.
///  2: [writable]          - PDA     : Shuttle EATA account (PDA derived from [shuttle_metadata, mint]).
///  3: [writable]          - SPL     : Shuttle wallet ATA account.
///  4: [writable]          - SPL     : Destination token account owned by the shuttle owner.
///  5: []                  - SPL     : Mint account.
///  6: []                  - PDA     : Global vault account.
///  7: [writable]          - SPL     : Vault source token account.
///  8: []                  - SPL     : Token program account.
///
/// Instruction Data: none
///
pub fn process_recover_and_close_shuttle_to_owner(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    require!(instruction_data.is_empty(), ProgramError::InvalidInstructionData);

    let [
        _rent_reimbursement_info, // force multi-line
        shuttle_info,
        shuttle_ephemeral_ata_info,
        _shuttle_wallet_ata_info,
        destination_token_info,
        mint_info,
        _vault_info,
        _vault_source_token_acc,
        token_program_info,
    ] = ephemeral_spl_api::require_n_accounts!(accounts, 9);

    require!(shuttle_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);
    require!(
        shuttle_ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let shuttle_owner = {
        let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        shuttle.owner
    };
    validate_token_account(
        destination_token_info,
        mint_info.address(),
        Some(&shuttle_owner),
        Some(token_program_info.address()),
    )?;

    settle_and_close_shuttle_accounts(accounts, None)
}
