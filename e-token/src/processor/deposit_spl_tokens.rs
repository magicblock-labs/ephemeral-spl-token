use core::marker::PhantomData;

use ephemeral_spl_api::state::{load_initialized, load_mut_initialized};
use ephemeral_spl_api::{require, require_n_accounts_with_ignored};

use {
    ephemeral_spl_api::state::ephemeral_ata::EphemeralAta,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

use crate::processor::internal::token_vault::transfer_to_vault_for_mint;

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Ephemeral ATA data account.
///  1: []                  - PDA     : Global vault account.
///  2: []                  - SPL     : Mint account.
///  3: [writable]          - SPL     : User source token account.
///  4: [writable]          - SPL     : Vault destination token account.
///  5: [signer]            - Keypair : User authority.
///  6: []                  - SPL     : Token program.
///
/// Instruction Data: DepositArgs
///
#[inline(always)]
pub fn process_deposit_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        ephemeral_ata_info, // force multi-line
        vault_info,
        mint_info,
        user_source_token_acc,
        vault_token_acc,
        user_authority,
        token_program_info,
    ] = require_n_accounts_with_ignored!(accounts, 7);

    let args = DepositArgs::try_from_bytes(instruction_data)?;

    // Validate EphemeralAta ownership first, before reading raw data.
    require!(
        ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
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

///
/// DataLayout:
///
///     00..08 : amount (u64)
///
/// ValidLength:
///
///     >= 08
///
pub struct DepositArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl DepositArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositArgs<'_>, ProgramError> {
        require!(bytes.len() >= 8, ProgramError::InvalidInstructionData);
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
