use ephemeral_spl_api::state::load_initialized;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts_with_ignored};
use pinocchio::cpi::Signer;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::utils::{read_mint_decimals, validate_token_account};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Owner (must match shuttle metadata owner).
///  1: [writable]          - SPL     : Destination SPL token account.
///  2: []                  - PDA     : Shuttle metadata account (PDA derived from [owner, mint, shuttle_id]).
///  3: [writable]          - SPL     : Shuttle wallet ATA.
///  4: []                  - SPL     : Mint account.
///  5: []                  - SPL     : Token program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_merge_shuttle_into_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        owner_info, // force multi-line
        destination_token_info,
        shuttle_info,
        shuttle_wallet_ata_info,
        mint_info,
        token_program_info,
    ] = require_n_accounts_with_ignored!(accounts, 6);

    require!(
        owner_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    require!(
        shuttle_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(
        shuttle_wallet_ata_info.owned_by(token_program_info.address()),
        ProgramError::InvalidAccountOwner
    );
    require!(
        destination_token_info.owned_by(token_program_info.address()),
        ProgramError::InvalidAccountOwner
    );

    let (shuttle_owner, shuttle_id, bump) = {
        let shuttle =
            load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        require_eq_keys!(
            &shuttle.owner,
            owner_info.address(),
            ProgramError::IncorrectAuthority
        );
        #[allow(clippy::clone_on_copy)]
        let owner = shuttle.owner.clone();
        (owner, shuttle.id, shuttle.bump)
    };

    let shuttle_id_seed = shuttle_id.to_le_bytes();
    let derived_shuttle =
        ShuttleMetadata::derive_pda(&shuttle_owner, mint_info.address(), shuttle_id, bump)?;
    require_eq_keys!(
        &derived_shuttle,
        shuttle_info.address(),
        ProgramError::InvalidSeeds
    );

    let shuttle_amount = {
        let shuttle_ata = validate_token_account(
            shuttle_wallet_ata_info,
            mint_info.address(),
            Some(shuttle_info.address()),
            Some(token_program_info.address()),
        )?;

        let amount = shuttle_ata.amount();
        if amount == 0 {
            return Ok(());
        }

        validate_token_account(
            destination_token_info,
            mint_info.address(),
            None,
            Some(token_program_info.address()),
        )?;

        amount
    };

    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let bump_seed = [bump];
    let seeds = ShuttleMetadata::signer_seeds(
        &shuttle_owner,
        mint_info.address(),
        &shuttle_id_seed,
        &bump_seed,
    );
    let signer = Signer::from(&seeds);

    pinocchio_token_2022::instructions::TransferChecked {
        mint: mint_info,
        from: shuttle_wallet_ata_info,
        to: destination_token_info,
        authority: shuttle_info,
        token_program: token_program_info.address(),
        amount: shuttle_amount,
        decimals,
    }
    .invoke_signed(&[signer])?;

    Ok(())
}
