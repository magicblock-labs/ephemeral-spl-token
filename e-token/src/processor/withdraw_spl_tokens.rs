use core::marker::PhantomData;
use pinocchio::instruction::{Seed, Signer};
use {
    ephemeral_spl_api::state::{
        ephemeral_ata::EphemeralAta, global_vault::GlobalVault, load_mut_unchecked, load_unchecked,
    },
    pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult},
};

#[inline(always)]
pub fn process_withdraw_spl_tokens(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA data account (PDA [payer, mint])
    // 1. []         Global Vault data account (PDA [mint])
    // 2. []         Mint account (readonly)
    // 3. [writable] Vault source token account (SPL Token)
    // 4. [writable] User destination token account (SPL Token)
    // 5. []         Token program

    let args = WithdrawArgs::try_from_bytes(instruction_data)?;

    let [ephemeral_ata_info, vault_info, mint_info, vault_source_token_acc, user_dest_token_acc, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Validate EphemeralAta account (writable)
    let ephemeral_ata = unsafe {
        load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_mut_data_unchecked())?
    };

    // Validate Vault data account
    let vault = unsafe { load_unchecked::<GlobalVault>(vault_info.borrow_data_unchecked())? };

    // Check mint consistency
    if ephemeral_ata.mint != *mint_info.key() || vault.mint != *mint_info.key() {
        return Err(ProgramError::InvalidAccountData);
    }

    // read mint decimals
    let decimals = pinocchio_token::state::Mint::from_account_info(mint_info)
        .map_err(|_| ProgramError::InvalidAccountData)?
        .decimals();

    // Perform transfer from vault token account to user destination, signed by vault PDA
    let bump = [args.bump()];
    let seeds = [Seed::from(mint_info.key().as_slice()), Seed::from(&bump)];
    let signer = Signer::from(&seeds);

    pinocchio_token::instructions::TransferChecked {
        mint: mint_info,
        from: vault_source_token_acc,
        to: user_dest_token_acc,
        amount: args.amount(),
        authority: vault_info, // PDA authority over the vault token account
        decimals,
    }
    .invoke_signed(&[signer])?;

    // Safely decrease the amount in the EphemeralAta
    ephemeral_ata.amount = ephemeral_ata
        .amount
        .checked_sub(args.amount())
        .ok_or(ProgramError::InvalidArgument)?;

    Ok(())
}

/// Instruction data for the `WithdrawSplTokens` instruction.
pub struct WithdrawArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl WithdrawArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<WithdrawArgs, ProgramError> {
        if bytes.len() < 9 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(WithdrawArgs {
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

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw.add(8) }
    }
}
