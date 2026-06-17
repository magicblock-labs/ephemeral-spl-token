use ephemeral_spl_api::require_n_accounts;
use pinocchio::{AccountView, ProgramResult};

use crate::processor::internal::ephemeral_ata::initialize_ephemeral_ata_with_sponsor;

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Ephemeral ATA account (PDA derived from [user, mint]).
///  1: []                  - Any     : Payer.
///  2: []                  - Any     : User.
///  3: []                  - SPL     : Mint.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_initialize_ephemeral_ata(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    let [
        ephemeral_ata_info, // force multi-line
        payer_info,
        user_info,
        mint_info,
        _system_program,
    ] = require_n_accounts!(accounts, 5);

    initialize_ephemeral_ata_with_sponsor(ephemeral_ata_info, payer_info, None, user_info, mint_info)
}
