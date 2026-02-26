use ephemeral_spl_api::state::{
    load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta, Initializable,
};
use pinocchio::cpi::Signer;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::state::{Mint, TokenAccount};

#[inline(always)]
pub fn process_merge_shuttle_into_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Owner
    // 1. [writable] Owner destination token account (SPL token account owned by owner)
    // 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id]) - must be program-owned
    // 3. [writable] Shuttle wallet ATA account (source SPL token account owned by shuttle PDA)
    // 4. []         Mint account
    // 5. []         Token program
    let [owner_info, owner_token_ata_info, shuttle_info, shuttle_wallet_ata_info, mint_info, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !owner_info.is_signer() {
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

    let (shuttle_owner, shuttle_id, shuttle_bump) = {
        let shuttle =
            unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
        if !shuttle.is_initialized() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let owner = shuttle.owner.clone();
        (owner, shuttle.id, shuttle.bump)
    };

    if shuttle_owner != *owner_info.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let shuttle_id_seed = shuttle_id.to_le_bytes();
    let derived_shuttle = ShuttleEphemeralAta::create_address(
        &shuttle_owner,
        &mint_info.address(),
        shuttle_id,
        &[shuttle_bump],
    )?;
    if derived_shuttle != *shuttle_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let decimals = {
        let mint_data = unsafe { mint_info.borrow_unchecked() };
        if mint_data.len() < Mint::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
        if !mint.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        mint.decimals()
    };

    let shuttle_amount = {
        let source_data = unsafe { shuttle_wallet_ata_info.borrow_unchecked() };
        if source_data.len() < TokenAccount::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let source = unsafe { TokenAccount::from_bytes_unchecked(source_data) };
        if !source.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if source.mint() != mint_info.address() || source.owner() != shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        source.amount()
    };

    {
        let destination_data = unsafe { owner_token_ata_info.borrow_unchecked() };
        if destination_data.len() < TokenAccount::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let destination = unsafe { TokenAccount::from_bytes_unchecked(destination_data) };
        if !destination.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if destination.mint() != mint_info.address() || destination.owner() != owner_info.address()
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    if shuttle_amount > 0 {
        let bump = [shuttle_bump];
        let seeds = ShuttleEphemeralAta::signer_seeds(
            &shuttle_owner,
            &mint_info.address(),
            &shuttle_id_seed,
            &bump,
        );
        let signer = Signer::from(&seeds);

        pinocchio_token_2022::instructions::TransferChecked {
            mint: mint_info,
            from: shuttle_wallet_ata_info,
            to: owner_token_ata_info,
            authority: shuttle_info,
            token_program: token_program_info.address(),
            amount: shuttle_amount,
            decimals,
        }
        .invoke_signed(&[signer])?;
    }

    Ok(())
}
