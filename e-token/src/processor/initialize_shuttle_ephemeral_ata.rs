use core::marker::PhantomData;
use ephemeral_spl_api::{require, require_n_accounts_with_ignored};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::ephemeral_ata::initialize_shuttle_ephemeral_ata_with_sponsor;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Shuttle metadata account (PDA derived from [owner, mint, shuttle_id]).
///  2: [writable]          - PDA     : Shuttle EATA account (PDA derived from [shuttle_metadata, mint]).
///  3: [writable]          - SPL     : Shuttle wallet ATA account (ATA for [shuttle_metadata, mint]).
///  4: []                  - Any     : Owner.
///  5: []                  - SPL     : Mint.
///  6: []                  - SPL     : Token program.
///  7: []                  - SPL     : Associated token program.
///  8: []                  - Builtin : System program.
///
/// Instruction Data: InitializeShuttleEphemeralAta
///
#[inline(always)]
pub fn process_initialize_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        owner_info,
        mint_info,
        token_program_info,
        _associated_token_program_info,
        system_program_info,
    ] = require_n_accounts_with_ignored!(accounts, 9);

    let args = InitializeShuttleEphemeralAta::try_from_bytes(instruction_data)?;

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    initialize_shuttle_ephemeral_ata_with_sponsor(
        payer_info,
        None,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        payer_info,
        owner_info,
        mint_info,
        token_program_info,
        system_program_info,
        args.shuttle_id(),
    )?;

    Ok(())
}

///
/// DataLayout:
///
///     00..04 : shuttle_id (u32)
///
/// ValidLength:
///
///     >= 04
///
pub struct InitializeShuttleEphemeralAta<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeShuttleEphemeralAta<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeShuttleEphemeralAta<'_>, ProgramError> {
        require!(bytes.len() >= 4, ProgramError::InvalidInstructionData);

        Ok(InitializeShuttleEphemeralAta {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn shuttle_id(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }
}
