use alloc::vec::Vec;

use dlp_api::compact::ClearText;
use ephemeral_spl_api::{instructions::DepositAndDelegateShuttleArgs, require_n_accounts};
use pinocchio::{AccountView, ProgramResult};
use solana_instruction::Instruction;
use wheels::layout::Decodable as _;

use crate::processor::internal::shuttle_delegation::{
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions, start_async_shuttle_close_action,
    sweep_shuttle_balance_action, DepositAndDelegateShuttleAccounts, DepositAndDelegateShuttleCommonArgs,
};

struct StartAsyncShuttleTransferAccounts<'a> {
    pub(crate) common: DepositAndDelegateShuttleAccounts<'a>,
    pub(crate) destination_token_info: &'a AccountView,
}

///
/// Executes on:
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
/// 13: [writable]          - SPL     : Destination token account.
/// 14: []                  - SPL     : Mint account.
/// 15: []                  - SPL     : Token program.
/// 16: []                  - PDA     : Global vault account.
/// 17: [writable]          - SPL     : Owner source token account.
/// 18: [writable]          - SPL     : Vault token account.
///
/// Instruction Data: DepositAndDelegateShuttleArgs
///
#[inline(never)]
pub fn process_start_async_shuttle_transfer(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        owner_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        _associated_token_program,
        system_program,
        destination_token_info,
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
    ] = require_n_accounts!(accounts, 19);

    let args = DepositAndDelegateShuttleArgs::decode(instruction_data)?;

    let accounts = StartAsyncShuttleTransferAccounts {
        common: DepositAndDelegateShuttleAccounts {
            payer_info,
            rent_pda_info,
            shuttle_info,
            shuttle_eata_info,
            shuttle_wallet_ata_info,
            owner_info,
            owner_program,
            buffer_acc,
            delegation_record,
            delegation_metadata,
            system_program,
            mint_info,
            token_program_info,
            global_vault_info,
            owner_source_token_info,
            vault_token_info,
        },
        destination_token_info,
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &accounts.common,
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: args.shuttle_id(),
            total_amount: args.amount(),
            validator: args.validator(),
        },
        0,
        default_post_delegation_actions(&accounts).cleartext(),
    )
}

fn default_post_delegation_actions(accounts: &StartAsyncShuttleTransferAccounts<'_>) -> Vec<Instruction> {
    alloc::vec![
        merge_shuttle_into_destination_action(accounts),
        start_async_shuttle_close_action(&accounts.common, None),
    ]
}

fn merge_shuttle_into_destination_action(accounts: &StartAsyncShuttleTransferAccounts<'_>) -> Instruction {
    sweep_shuttle_balance_action(&accounts.common, accounts.destination_token_info)
}
