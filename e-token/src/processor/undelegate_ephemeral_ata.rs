use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked};
use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::find_program_address,
    ProgramResult,
};

/// Undelegate an Ephemeral ATA by calling into the delegation program helper that
/// schedules a commit and performs undelegation.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Payer
/// 1. [writable] Ephemeral ATA account (PDA derived from [payer, mint])
/// 2. [writable] Magic context account (as required by the delegation program)
/// 3. []         Delegation program ID (aka magic program)
pub fn process_undelegate_ephemeral_ata(
    accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer, ephemeral_ata_info, magic_context, magic_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Ensure the payer signed the transaction
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Read the Ephemeral ATA to get the mint and verify the PDA derivation for this payer
    let ata_data =
        unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_data_unchecked())? };

    // Derive PDA: seeds = [payer, mint], program id = e-token program id (ephemeral_spl_api::program::ID)
    let (derived_pda, _) = find_program_address(
        &[payer.key().as_slice(), ata_data.mint.as_slice()],
        &ephemeral_spl_api::program::ID,
    );

    if derived_pda != *ephemeral_ata_info.key() {
        return Err(ProgramError::InvalidSeeds);
    }

    // Commit and undelegate with the ephemeral ATA as the account set
    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        &[ephemeral_ata_info.clone()],
        magic_context,
        magic_program,
    )
}
