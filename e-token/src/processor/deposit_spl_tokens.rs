use core::marker::PhantomData;
use {
    ephemeral_spl_api::state::{
        ephemeral_ata::EphemeralAta, global_vault::GlobalVault, load_mut_unchecked, load_unchecked,
    },
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
    pinocchio_token_2022::state::Mint,
};

#[inline(always)]
pub fn process_deposit_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA data account (PDA [payer, mint])
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
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    // Validate EphemeralAta account
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };

    // Validate vault ownership first, before reading raw data.
    unsafe {
        if vault_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    // Validate Vault data account
    let vault = unsafe { load_unchecked::<GlobalVault>(vault_info.borrow_unchecked())? };

    // Check mint consistency
    if ephemeral_ata.mint != *mint_info.address()
        || vault.mint != *mint_info.address()
        || vault.token_account != *vault_token_acc.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // Perform the actual SPL Token transfer via CPI using custom token transfer.
    // Parse the base mint layout shared by both legacy SPL Token and Token-2022.
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
        amount: args.amount(),
        decimals,
    }
    .invoke()?;

    // Safely increase the amount in the EphemeralAta
    ephemeral_ata.amount = ephemeral_ata
        .amount
        .checked_add(args.amount())
        .ok_or(ProgramError::InvalidArgument)?;

    Ok(())
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
