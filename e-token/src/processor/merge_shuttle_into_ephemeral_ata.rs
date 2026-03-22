use ephemeral_spl_api::state::{
    load_unchecked, shuttle_ephemeral_ata::ShuttleMetadata, Initializable,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_token_2022::state::{Mint, TokenAccount};

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

    assert_owner(shuttle_info, &ephemeral_spl_api::ID)?;
    assert_owner(shuttle_wallet_ata_info, token_program_info.address())?;
    assert_owner(destination_token_info, token_program_info.address())?;

    let (shuttle_owner, shuttle_id, bump) = {
        let shuttle =
            unsafe { load_unchecked::<ShuttleMetadata>(shuttle_info.borrow_unchecked())? };
        if !shuttle.is_initialized() {
            return Err(ProgramError::InvalidAccountData);
        }
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
        &ephemeral_spl_api::ID,
    )?;
    if derived_shuttle != *shuttle_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let (shuttle_mint, shuttle_wallet_owner, shuttle_amount) =
        read_token_account(shuttle_wallet_ata_info)?;
    if shuttle_wallet_owner != *shuttle_info.address() || shuttle_mint != *mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let (destination_mint, _destination_owner, _destination_amount) =
        read_token_account(destination_token_info)?;
    if destination_mint != *mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    if shuttle_amount == 0 {
        return Ok(());
    }

    let decimals = read_mint_decimals(mint_info)?;

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

#[inline(always)]
fn assert_owner(account: &AccountView, expected_owner: &Address) -> ProgramResult {
    unsafe {
        if account.owner().ne(expected_owner) {
            return Err(ProgramError::IllegalOwner);
        }
    }
    Ok(())
}

#[inline(always)]
fn read_token_account(account: &AccountView) -> Result<(Address, Address, u64), ProgramError> {
    let token_data = unsafe { account.borrow_unchecked() };
    if token_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let token = unsafe { TokenAccount::from_bytes_unchecked(token_data) };
    if !token.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }

    #[allow(clippy::clone_on_copy)]
    let mint = token.mint().clone();
    #[allow(clippy::clone_on_copy)]
    let owner = token.owner().clone();
    Ok((mint, owner, token.amount()))
}

#[inline(always)]
fn read_mint_decimals(mint_info: &AccountView) -> Result<u8, ProgramError> {
    let mint_data = unsafe { mint_info.borrow_unchecked() };
    if mint_data.len() < Mint::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
    if !mint.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }

    Ok(mint.decimals())
}
