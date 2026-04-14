use crate::{assert_owner, assert_signer, processor::utils::read_mint_decimals};
use ephemeral_spl_api::{error::EphemeralSplError, state::load_initialized};
use pinocchio::cpi::Signer;

use {
    ephemeral_spl_api::state::{
        ephemeral_ata::load_ephemeral_ata_compat_mut, global_vault::GlobalVault,
    },
    pinocchio::{error::ProgramError, AccountView, Address, ProgramResult},
};

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn transfer_to_vault_for_mint(
    vault_info: &AccountView,
    mint_info: &AccountView,
    user_source_token_acc: &AccountView,
    vault_token_acc: &AccountView,
    user_authority: &AccountView,
    token_program_info: &AccountView,
    expected_mint: &Address,
    amount: u64,
) -> ProgramResult {
    assert_owner!(vault_info, &crate::ID);

    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;
    if vault.mint != *mint_info.address()
        || vault.token_account != *vault_token_acc.address()
        || vault.mint != *expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    pinocchio_token_2022::instructions::TransferChecked {
        mint: mint_info,
        from: user_source_token_acc,
        to: vault_token_acc,
        authority: user_authority,
        token_program: token_program_info.address(),
        amount,
        decimals,
    }
    .invoke()
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn withdraw_ephemeral_ata_tokens(
    owner: &AccountView,
    require_owner_signature: bool,
    ephemeral_ata_info: &AccountView,
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_source_token_acc: &AccountView,
    user_dest_token_acc: &AccountView,
    token_program_info: &AccountView,
    amount: u64,
) -> ProgramResult {
    if require_owner_signature {
        assert_signer!(owner);
    }

    // Validate EphemeralAta account (writable)
    assert_owner!(ephemeral_ata_info, &crate::ID);
    let mut ephemeral_ata =
        load_ephemeral_ata_compat_mut(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;

    // Validate vault ownership before reading raw data.
    assert_owner!(vault_info, &crate::ID);

    // Validate Vault data account
    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;

    // Check eata consistency
    if ephemeral_ata.mint() != mint_info.address()
        || vault.mint != *mint_info.address()
        || ephemeral_ata.owner() != owner.address()
        || vault.token_account != *vault_source_token_acc.address()
    {
        return Err(EphemeralSplError::EphemeralAtaMismatch.into());
    }

    // Parse the base mint layout shared by both legacy SPL Token and Token-2022.
    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    // Perform transfer from vault token account to user destination, signed by vault PDA
    let bump = [vault.bump];
    let seeds = GlobalVault::signer_seeds(mint_info.address(), &bump);
    let signer = Signer::from(&seeds);

    pinocchio_token_2022::instructions::TransferChecked {
        mint: mint_info,
        from: vault_source_token_acc,
        to: user_dest_token_acc,
        authority: vault_info, // PDA authority over the vault token account
        token_program: token_program_info.address(),
        amount,
        decimals,
    }
    .invoke_signed(&[signer])?;

    // Safely decrease the amount in the EphemeralAta
    let updated_amount = ephemeral_ata
        .amount()
        .checked_sub(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    ephemeral_ata.set_amount(updated_amount);

    Ok(())
}

#[inline(always)]
pub(crate) fn validate_vault_for_mint(
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_token_acc_info: &AccountView,
) -> Result<u8, ProgramError> {
    assert_owner!(vault_info, &crate::ID);

    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;
    let derived_vault = GlobalVault::derive_pda(mint_info.address(), vault.bump)?;
    if derived_vault != *vault_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if vault.mint != *mint_info.address() || vault.token_account != *vault_token_acc_info.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(vault.bump)
}
