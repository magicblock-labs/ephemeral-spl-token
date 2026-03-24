use ephemeral_spl_api::state::load_initialized;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use pinocchio::cpi::{Seed, Signer};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::assert_owner;
use crate::processor::utils::{read_mint_decimals, validate_token_account};

#[inline(always)]
pub fn process_merge_shuttle_into_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Owner (must match shuttle metadata owner)
    // 1. [writable] Destination SPL token account
    // 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id]) - must be program-owned
    // 3. [writable] Shuttle wallet ATA (source SPL token account owned by shuttle PDA)
    // 4. []         Mint account
    // 5. []         Token program
    let [owner_info, destination_token_info, shuttle_info, shuttle_wallet_ata_info, mint_info, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !owner_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    assert_owner!(shuttle_info, &ephemeral_spl_api::program::id_address());
    assert_owner!(shuttle_wallet_ata_info, token_program_info.address());
    assert_owner!(destination_token_info, token_program_info.address());

    let (shuttle_owner, shuttle_id, bump) = {
        let shuttle =
            load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        if shuttle.owner != *owner_info.address() {
            return Err(ProgramError::IncorrectAuthority);
        }
        #[allow(clippy::clone_on_copy)]
        let owner = shuttle.owner.clone();
        (owner, shuttle.id, [shuttle.bump])
    };

    let shuttle_id_seed = shuttle_id.to_le_bytes();
    let derived_shuttle = ephemeral_spl_api::Address::create_program_address(
        &[
            shuttle_owner.as_ref(),
            mint_info.address().as_ref(),
            shuttle_id_seed.as_ref(),
            bump.as_ref(),
        ],
        &ephemeral_spl_api::program::id_address(),
    )?;
    if derived_shuttle != *shuttle_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let (_, _, shuttle_amount) = validate_token_account(
        shuttle_wallet_ata_info,
        mint_info.address(),
        Some(shuttle_info.address()),
        Some(token_program_info.address()),
    )?;

    if shuttle_amount == 0 {
        return Ok(());
    }

    validate_token_account(
        destination_token_info,
        mint_info.address(),
        None,
        Some(token_program_info.address()),
    )?;

    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let seeds = [
        Seed::from(shuttle_owner.as_ref()),
        Seed::from(mint_info.address().as_ref()),
        Seed::from(shuttle_id_seed.as_ref()),
        Seed::from(&bump),
    ];
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
