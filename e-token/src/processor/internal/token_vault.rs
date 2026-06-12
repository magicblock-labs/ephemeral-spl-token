use ephemeral_spl_api::{
    error::EphemeralSplError,
    require, require_eq_keys,
    state::{
        ephemeral_ata::load_ephemeral_ata_compat_mut,
        global_vault::GlobalVault,
        load_initialized,
        transfer_queue::{queue_views_checked, TransferQueue},
    },
};
use pinocchio::{cpi::Signer, error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::internal::{get_associated_token_address, read_mint_decimals};

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
    require!(vault_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);

    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;
    require!(
        vault.mint == *mint_info.address()
            && vault.token_account == *vault_token_acc.address()
            && vault.mint == *expected_mint,
        ProgramError::InvalidAccountData
    );

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

#[allow(clippy::too_many_arguments)]
pub(crate) fn transfer_to_queue_vault_for_mint(
    queue_info: &AccountView,
    mint_info: &AccountView,
    user_source_token_acc: &AccountView,
    vault_token_acc: &AccountView,
    user_authority: &AccountView,
    token_program_info: &AccountView,
    expected_mint: &Address,
    amount: u64,
) -> ProgramResult {
    let _ = validate_queue_vault_for_mint(
        queue_info,
        mint_info,
        vault_token_acc,
        token_program_info,
        expected_mint,
    )?;

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
        require!(owner.is_signer(), ProgramError::MissingRequiredSignature);
    }

    // Validate EphemeralAta account (writable)
    require!(
        ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );
    let mut ephemeral_ata = load_ephemeral_ata_compat_mut(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;

    // Validate vault ownership before reading raw data.
    require!(vault_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);

    // Validate Vault data account
    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;

    require!(
        ephemeral_ata.owner() == owner.address(),
        EphemeralSplError::EphemeralAtaMismatch
    );
    require!(
        ephemeral_ata.mint() == mint_info.address() && vault.mint == *mint_info.address(),
        EphemeralSplError::MintMismatch
    );
    require!(
        vault.token_account == *vault_source_token_acc.address(),
        EphemeralSplError::VaultTokenAccountMismatch
    );

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

pub(crate) struct QueueVault {
    pub(crate) bump: u8,
    pub(crate) validator: Address,
}

#[inline(always)]
pub(crate) fn validate_vault_for_mint(
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_token_acc_info: &AccountView,
) -> Result<u8, ProgramError> {
    require!(vault_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);

    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;
    let derived_vault = GlobalVault::derive_pda(mint_info.address(), vault.bump)?;
    require_eq_keys!(&derived_vault, vault_info.address(), ProgramError::InvalidSeeds);
    require!(
        vault.mint == *mint_info.address() && vault.token_account == *vault_token_acc_info.address(),
        ProgramError::InvalidAccountData
    );

    Ok(vault.bump)
}

pub(crate) fn validate_queue_vault_for_mint(
    queue_info: &AccountView,
    mint_info: &AccountView,
    vault_token_acc_info: &AccountView,
    token_program_info: &AccountView,
    expected_mint: &Address,
) -> Result<QueueVault, ProgramError> {
    let (bump, validator) = {
        let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
        require!(
            header.mint == *mint_info.address() && header.mint == *expected_mint,
            ProgramError::InvalidAccountData
        );
        (header.bump, header.validator)
    };

    let derived_queue = TransferQueue::derive_pda(mint_info.address(), &validator, bump)?;
    require_eq_keys!(&derived_queue, queue_info.address(), ProgramError::InvalidSeeds);

    let expected_vault_token =
        get_associated_token_address(queue_info.address(), mint_info.address(), token_program_info.address());
    require_eq_keys!(
        &expected_vault_token,
        vault_token_acc_info.address(),
        EphemeralSplError::VaultTokenAccountMismatch
    );

    Ok(QueueVault { bump, validator })
}
