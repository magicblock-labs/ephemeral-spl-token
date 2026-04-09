use core::marker::PhantomData;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::token_vault::withdraw_ephemeral_ata_tokens;

#[inline(always)]
pub fn process_withdraw_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Owner (payer, authority to withdraw)
    // 1. [writable] Ephemeral ATA data account (PDA [owner, mint])
    // 2. []         Global Vault data account (PDA [mint])
    // 3. []         Mint account (readonly)
    // 4. [writable] Vault source token account (SPL Token)
    // 5. [writable] User destination token account (SPL Token)
    // 6. []         Token program

    let args = WithdrawArgs::try_from_bytes(instruction_data)?;

    let [owner, ephemeral_ata_info, vault_info, mint_info, vault_source_token_acc, user_dest_token_acc, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

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
        if bytes.len() != 8 {
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
}
