use core::marker::PhantomData;
use ephemeral_spl_api::{require, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::token_vault::withdraw_ephemeral_ata_tokens;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Owner.
///  1: [writable]          - PDA     : Ephemeral ATA data account.
///  2: []                  - PDA     : Global vault account.
///  3: []                  - SPL     : Mint account.
///  4: [writable]          - SPL     : Vault source token account.
///  5: [writable]          - SPL     : User destination token account.
///  6: []                  - SPL     : Token program.
///
/// Instruction Data: WithdrawArgs
///
#[inline(always)]
pub fn process_withdraw_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        owner, // force multi-line
        ephemeral_ata_info,
        vault_info,
        mint_info,
        vault_source_token_acc,
        user_dest_token_acc,
        token_program_info,
    ] = require_n_accounts!(accounts, 7);

    let args = WithdrawArgs::try_from_bytes(instruction_data)?;

    withdraw_ephemeral_ata_tokens(
        owner,
        true,
        ephemeral_ata_info,
        vault_info,
        mint_info,
        vault_source_token_acc,
        user_dest_token_acc,
        token_program_info,
        args.amount(),
    )
}

///
/// DataLayout:
///
///     00..08 : amount (u64)
///
/// ValidLength:
///
///     08
///
pub struct WithdrawArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl WithdrawArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<WithdrawArgs<'_>, ProgramError> {
        require!(bytes.len() == 8, ProgramError::InvalidInstructionData);
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
}
