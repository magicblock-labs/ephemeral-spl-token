use ephemeral_spl_api::instructions::DepositAndDelegateShuttleWithPrivateTransferArgs;
use pinocchio::{AccountView, ProgramResult};
use wheels::layout::Decodable as _;

use crate::processor::internal::private_transfer::process_with_merge_and_private_transfer_inner;

///
/// Executes on: BASE only.
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Shuttle metadata account.
///  3: [writable]          - PDA     : Shuttle EATA account.
///  4: [writable]          - SPL     : Shuttle wallet ATA account.
///  5: [signer]            - Keypair : Shuttle owner.
///  6: []                  - Program : Owner program.
///  7: [writable]          - PDA     : Buffer account.
///  8: [writable]          - PDA     : Delegation record account.
///  9: [writable]          - PDA     : Delegation metadata account.
/// 10: []                  - Program : Delegation program.
/// 11: []                  - SPL     : Associated token program.
/// 12: []                  - Builtin : System program.
/// 13: []                  - SPL     : Mint account.
/// 14: []                  - SPL     : Token program.
/// 15: []                  - PDA     : Global vault account.
/// 16: [writable]          - SPL     : Owner source token account.
/// 17: [writable]          - SPL     : Vault token account.
/// 18: [writable]          - PDA     : Transfer queue account.
///
/// Instruction Data: DepositAndDelegateShuttleWithPrivateTransferArgs
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DepositAndDelegateShuttleWithPrivateTransferArgs::decode(instruction_data)?;
    process_with_merge_and_private_transfer_inner(
        accounts,
        args.shuttle_id(),
        args.amount(),
        args.exact_out(),
        args.validator(),
        args.encrypted_destination(),
        args.encrypted_data_suffix(),
        None,
    )
}
