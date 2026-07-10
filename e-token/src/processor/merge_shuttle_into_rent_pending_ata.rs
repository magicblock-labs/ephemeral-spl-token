use ephemeral_spl_api::{require, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::processor::{
    internal::rent_pending_destination::{ensure_rent_pending_destination, RentPendingDestinationAccounts},
    merge_shuttle_into_ephemeral_ata::merge_shuttle_balance,
};

///
/// Executes on: ER only (decrypted post-delegation action of instruction 32).
///
/// Creates the destination ATA as an idempotent rent-pending ATA through the
/// Magic program, then merges the whole shuttle wallet ATA balance into it.
/// The rent-pending ATA must end the transaction with a positive amount, which
/// the merge guarantees since instruction 32 rejects zero amounts. Read access
/// is owner-gated by the private RPC's token-account default.
///
/// Accounts:
///
///  0: [signer]            - Keypair : Owner (must match shuttle metadata owner).
///  1: []                  - Keypair : Destination owner (decrypted).
///  2: [writable]          - SPL     : Destination ATA (decrypted).
///  3: []                  - PDA     : Shuttle metadata account.
///  4: [writable]          - SPL     : Shuttle wallet ATA.
///  5: []                  - SPL     : Mint account.
///  6: []                  - SPL     : Token program.
///  7: []                  - Builtin : Magic program.
///
/// Instruction Data: None
///
#[inline(never)]
pub fn process_merge_shuttle_into_rent_pending_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        owner_info, // force multi-line
        destination_owner_info,
        destination_ata_info,
        shuttle_info,
        shuttle_wallet_ata_info,
        mint_info,
        token_program_info,
        magic_program_info,
    ] = require_n_accounts!(accounts, 8);

    require!(owner_info.is_signer(), ProgramError::MissingRequiredSignature);

    ensure_rent_pending_destination(&RentPendingDestinationAccounts {
        payer_info: owner_info,
        destination_owner_info,
        destination_ata_info,
        mint_info,
        token_program_info,
        magic_program_info,
    })?;

    merge_shuttle_balance(
        owner_info,
        destination_ata_info,
        shuttle_info,
        shuttle_wallet_ata_info,
        mint_info,
        token_program_info,
    )
}
