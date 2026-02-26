use core::marker::PhantomData;
use {
    ephemeral_spl_api::state::{
        ephemeral_ata::EphemeralAta, global_vault::GlobalVault, load_mut_unchecked, load_unchecked,
    },
    pinocchio::{error::ProgramError, AccountView, Address, ProgramResult},
    pinocchio_token_2022::state::Mint,
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
    unsafe {
        if ephemeral_ata_info
            .owner()
            .ne(&ephemeral_spl_api::program::ID)
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let ephemeral_ata_mint = {
        let ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked())? };
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        mint
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
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };
    ephemeral_ata.amount = ephemeral_ata
        .amount
        .checked_add(args.amount())
        .ok_or(ProgramError::InvalidArgument)?;

    Ok(())
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn transfer_to_vault_for_mint(
    vault_info: &AccountView,
    mint_info: &AccountView,
    user_source_token_acc: &AccountView,
    vault_token_acc: &AccountView,
    user_authority: &AccountView,
    token_program_info: &AccountView,
    expected_mint: &Address,
    amount: u64,
) -> ProgramResult {
    unsafe {
        if vault_info.owner().ne(&ephemeral_spl_api::program::ID) {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let vault = unsafe { load_unchecked::<GlobalVault>(vault_info.borrow_unchecked())? };
    if vault.mint != *mint_info.address()
        || vault.token_account != *vault_token_acc.address()
        || vault.mint != *expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
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
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositArgs, ProgramError> {
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
