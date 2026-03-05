use pinocchio::error::ProgramError;
use pinocchio::{AccountView, Address};
use pinocchio_token_2022::state::{Mint, TokenAccount};

#[inline(always)]
pub fn read_token_account(account: &AccountView) -> Result<(Address, Address, u64), ProgramError> {
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
pub fn read_mint_decimals(mint_info: &AccountView) -> Result<u8, ProgramError> {
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
