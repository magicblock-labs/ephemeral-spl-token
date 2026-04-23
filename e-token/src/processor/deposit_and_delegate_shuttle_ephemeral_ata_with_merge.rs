use alloc::vec::Vec;
use dlp_api::compact::ClearText;
use ephemeral_spl_api::require_n_accounts;
use pinocchio::{AccountView, ProgramResult};
use solana_instruction::Instruction;

use crate::processor::internal::shuttle_delegation::{
    merge_shuttle_into_token_account_action,
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
    undelegate_and_close_shuttle_action, DepositAndDelegateShuttleAccounts,
    DepositAndDelegateShuttleArgs,
};

struct DepositAndDelegateShuttleWithMergeAccounts<'a> {
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
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
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

    let accounts = DepositAndDelegateShuttleWithMergeAccounts {
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
        args.common_args(),
        0,
        default_post_delegation_actions(&accounts).cleartext(),
    )
}

fn default_post_delegation_actions(
    accounts: &DepositAndDelegateShuttleWithMergeAccounts<'_>,
) -> Vec<Instruction> {
    alloc::vec![
        merge_shuttle_into_destination_action(accounts),
        undelegate_and_close_shuttle_action(&accounts.common),
    ]
}

fn merge_shuttle_into_destination_action(
    accounts: &DepositAndDelegateShuttleWithMergeAccounts<'_>,
) -> Instruction {
    merge_shuttle_into_token_account_action(&accounts.common, accounts.destination_token_info)
}
