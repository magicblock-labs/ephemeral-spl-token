use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_unchecked};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token::state::TokenAccount;

/// Undelegate an Ephemeral ATA by calling into the delegation program helper that
/// schedules a commit and performs undelegation.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Payer
/// 1. [writable] User ATA account (SPL ATA for [payer, mint])
/// 2. [writable] Ephemeral ATA account (PDA derived from [payer, mint])
/// 3. [writable] Magic context account (as required by the delegation program)
/// 4. []         Delegation program ID (aka magic program)
pub fn process_undelegate_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [payer, ata_info, ephemeral_ata_info, magic_context, magic_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Ensure the payer signed the transaction
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Read the Ephemeral ATA to get the mint and verify the PDA derivation for this payer.
    // Scope the borrow so it's released before any CPI.
    let mint = {
        let eata_data =
            unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked())? };
        #[allow(clippy::clone_on_copy)]
        let mint = eata_data.mint.clone();
        mint
    };

    // Derive PDA: seeds = [payer, mint], program id = e-token program id (ephemeral_spl_api::program::ID)
    let (derived_pda, _) = ephemeral_spl_api::Address::find_program_address(
        &[payer.address().as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );

    if derived_pda != *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    // Validate that the provided ATA account is a valid SPL token account for [payer, mint].
    {
        let token_acc = TokenAccount::from_account_view(ata_info)
            .map_err(|_| ProgramError::InvalidAccountData)?;

        if token_acc.mint() != &mint || token_acc.owner() != payer.address() {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    // Commit and undelegate with the user's ATA and the ephemeral ATA as the account set
    ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
        payer,
        &[ata_info.clone()],
        magic_context,
        magic_program,
    )
}
