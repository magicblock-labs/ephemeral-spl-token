use bytemuck::{Pod, Zeroable};
use ephemeral_spl_api::{require_n_accounts, PodView};
use pinocchio::{AccountView, ProgramResult};

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

    let args = WithdrawArgs::try_view_from(instruction_data)?;

    withdraw_ephemeral_ata_tokens(
        owner,
        true,
        ephemeral_ata_info,
        vault_info,
        mint_info,
        vault_source_token_acc,
        user_dest_token_acc,
        token_program_info,
        args.amount,
    )
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct WithdrawArgs {
    amount: u64,
}
