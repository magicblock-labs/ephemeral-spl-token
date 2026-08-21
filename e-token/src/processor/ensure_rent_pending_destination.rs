use ephemeral_spl_api::{require, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::internal::rent_pending_destination::{
    ensure_rent_pending_destination, RentPendingDestinationAccounts,
};

///
/// Executes on: ER only.
///
/// Idempotently creates the destination's ATA as a rent-pending ATA through
/// the Magic program, so a plain SPL transfer in the same transaction can fund
/// a destination that does not exist yet. The rent-pending ATA must end the
/// transaction with a positive amount, so this instruction must be paired with
/// a funding transfer. Read access is owner-gated by the private RPC's
/// token-account default.
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: []                  - Keypair : Destination owner.
///  2: [writable]          - SPL     : Destination ATA.
///  3: []                  - SPL     : Mint account.
///  4: []                  - SPL     : Token program.
///  5: []                  - Builtin : Magic program.
///
/// Instruction Data: None
///
#[inline(never)]
pub fn process_ensure_rent_pending_destination(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    let [
        payer_info, // force multi-line
        destination_owner_info,
        destination_ata_info,
        mint_info,
        token_program_info,
        magic_program_info,
    ] = require_n_accounts!(accounts, 6);

    require!(payer_info.is_signer(), ProgramError::MissingRequiredSignature);

    ensure_rent_pending_destination(&RentPendingDestinationAccounts {
        payer_info,
        destination_owner_info,
        destination_ata_info,
        mint_info,
        token_program_info,
        magic_program_info,
    })
}
