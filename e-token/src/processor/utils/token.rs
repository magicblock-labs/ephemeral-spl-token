use ephemeral_spl_api::require;
use ephemeral_spl_api::state::transfer_queue::{
    TRANSFER_QUEUE_TOKEN_PROGRAM_SPL_TOKEN, TRANSFER_QUEUE_TOKEN_PROGRAM_TOKEN_2022,
};
use pinocchio::{address::address_eq, error::ProgramError, AccountView, Address};
use pinocchio_token_2022::state::{Mint, TokenAccount};
use spl_token_interface::ID as SPL_TOKEN_PROGRAM_ID;

#[inline(always)]
pub(crate) fn is_supported_token_program(token_program: &Address) -> bool {
    address_eq(token_program, &SPL_TOKEN_PROGRAM_ID)
        || address_eq(token_program, &pinocchio_token_2022::ID)
}

#[inline(always)]
pub(crate) fn token_program_kind(token_program: &Address) -> Result<u8, ProgramError> {
    if address_eq(token_program, &SPL_TOKEN_PROGRAM_ID) {
        Ok(TRANSFER_QUEUE_TOKEN_PROGRAM_SPL_TOKEN)
    } else if address_eq(token_program, &pinocchio_token_2022::ID) {
        Ok(TRANSFER_QUEUE_TOKEN_PROGRAM_TOKEN_2022)
    } else {
        Err(ProgramError::IncorrectProgramId)
    }
}

#[inline(always)]
pub(crate) fn token_program_for_kind(kind: u8) -> Result<Address, ProgramError> {
    match kind {
        TRANSFER_QUEUE_TOKEN_PROGRAM_SPL_TOKEN => Ok(SPL_TOKEN_PROGRAM_ID),
        TRANSFER_QUEUE_TOKEN_PROGRAM_TOKEN_2022 => Ok(pinocchio_token_2022::ID),
        _ => Err(ProgramError::InvalidAccountData),
    }
}

#[inline(always)]
pub(crate) fn read_mint_decimals(
    mint_info: &AccountView,
    token_program_info: &AccountView,
) -> Result<u8, ProgramError> {
    let mint_data = unsafe { mint_info.borrow_unchecked() };
    require!(
        mint_data.len() >= Mint::BASE_LEN,
        ProgramError::InvalidAccountData
    );
    require!(
        mint_info.owned_by(token_program_info.address()),
        ProgramError::InvalidAccountOwner
    );
    let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
    require!(mint.is_initialized(), ProgramError::UninitializedAccount);
    Ok(mint.decimals())
}

#[inline(always)]
pub fn read_token_account(account: &AccountView) -> Result<&TokenAccount, ProgramError> {
    let token_data = unsafe { account.borrow_unchecked() };
    require!(
        token_data.len() >= TokenAccount::BASE_LEN,
        ProgramError::InvalidAccountData
    );

    let token = unsafe { TokenAccount::from_bytes_unchecked(token_data) };
    require!(token.is_initialized(), ProgramError::UninitializedAccount);

    Ok(token)
}

#[inline(always)]
pub(crate) fn validate_token_account<'a>(
    ata_info: &'a AccountView,
    expected_mint: &Address,
    expected_owner: Option<&Address>,
    expected_token_program: Option<&Address>,
) -> Result<&'a TokenAccount, ProgramError> {
    if let Some(token_program) = expected_token_program {
        require!(
            ata_info.owned_by(token_program),
            ProgramError::InvalidAccountOwner
        );
    }

    let token = read_token_account(ata_info)?;
    require!(
        address_eq(token.mint(), expected_mint),
        ProgramError::InvalidAccountData
    );
    if let Some(expected_owner) = expected_owner {
        require!(
            address_eq(token.owner(), expected_owner),
            ProgramError::IllegalOwner
        );
    }

    Ok(token)
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
