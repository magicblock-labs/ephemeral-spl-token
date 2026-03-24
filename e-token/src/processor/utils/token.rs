use pinocchio::{address::address_eq, error::ProgramError, AccountView, Address};
use pinocchio_token_2022::state::{Mint, TokenAccount};

use crate::assert_owner;

pub type ReadTokenAccount = (Address, Address, u64);

#[inline(always)]
pub(crate) fn read_mint_decimals(
    mint_info: &AccountView,
    token_program_info: &AccountView,
) -> Result<u8, ProgramError> {
    let mint_data = unsafe { mint_info.borrow_unchecked() };
    if mint_data.len() < Mint::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    if !mint_info.owned_by(token_program_info.address()) {
        return Err(ProgramError::InvalidAccountOwner);
    }
    let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
    if !mint.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }
    Ok(mint.decimals())
}

#[inline(always)]
pub fn read_token_account(account: &AccountView) -> Result<ReadTokenAccount, ProgramError> {
    let token_data = unsafe { account.borrow_unchecked() };
    if token_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let token = unsafe { TokenAccount::from_bytes_unchecked(token_data) };
    if !token.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }

    Ok((*token.mint(), *token.owner(), token.amount()))
}

#[inline(always)]
pub(crate) fn validate_token_account(
    ata_info: &AccountView,
    expected_mint: &Address,
    expected_owner: Option<&Address>,
    expected_token_program: Option<&Address>,
) -> Result<ReadTokenAccount, ProgramError> {
    if let Some(token_program) = expected_token_program {
        assert_owner!(ata_info, token_program);
    }

    let (mint, owner, amount) = read_token_account(ata_info)?;
    if !address_eq(&mint, expected_mint) {
        return Err(ProgramError::InvalidAccountData);
    }
    if let Some(expected_owner) = expected_owner {
        if !address_eq(&owner, expected_owner) {
            return Err(ProgramError::IllegalOwner);
        }
    }

    Ok((mint, owner, amount))
}

#[inline(always)]
pub fn get_associated_token_address(
    wallet: &Address,
    mint: &Address,
    token_program: &Address,
) -> Address {
    ephemeral_spl_api::Address::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &pinocchio_associated_token_account::ID,
    )
    .0
}
