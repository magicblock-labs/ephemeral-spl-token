use ephemeral_spl_api::state::{load_initialized, load_mut_initialized};

use core::marker::PhantomData;

use crate::{assert_owner, processor::utils::read_mint_decimals};

use {
    ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, global_vault::GlobalVault},
    pinocchio::{error::ProgramError, AccountView, Address, ProgramResult},
};

#[inline(always)]
pub fn process_deposit_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA data account (standard EATA or shuttle EATA)
    // 1. []         Global Vault data account (PDA [mint])
    // 2. []         Mint account (readonly)
    // 3. [writable] User source token account (SPL Token)
    // 4. [writable] Vault destination token account (SPL Token)
    // 5. [signer]   User authority (owner of source token account)
    // 6. []         Token program

    let args = DepositArgs::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, vault_info, mint_info, user_source_token_acc, vault_token_acc, user_authority, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Validate EphemeralAta ownership first, before reading raw data.
    assert_owner!(
        ephemeral_ata_info,
        &ephemeral_spl_api::program::id_address()
    );

    let ephemeral_ata_mint = {
        let ephemeral_ata =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        ephemeral_ata.mint
    };

    transfer_to_vault_for_mint(
        vault_info,
        mint_info,
        user_source_token_acc,
        vault_token_acc,
        user_authority,
        token_program_info,
        &ephemeral_ata_mint,
        args.amount(),
    )?;

    let ephemeral_ata =
        load_mut_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;
    ephemeral_ata.amount = ephemeral_ata
        .amount
        .checked_add(args.amount())
        .ok_or(ProgramError::InvalidArgument)?;

    Ok(())
}

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
    assert_owner!(vault_info, &ephemeral_spl_api::program::id_address());

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

/// Instruction data for the `DepositSplTokens` instruction.
pub struct DepositArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl DepositArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositArgs<'_>, ProgramError> {
        if bytes.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(DepositArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        // read LE u64 from bytes[0..8]
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }
}
