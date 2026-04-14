use alloc::vec::Vec;
use dlp_api::compact::ClearText;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
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

#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DepositAndDelegateShuttleArgs::try_from_bytes(instruction_data)?;
    let accounts = parse_deposit_and_delegate_shuttle_with_merge_accounts(accounts)?;

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &accounts.common,
        args.common_args(),
        0,
        default_post_delegation_actions(&accounts).cleartext(),
    )
}

fn parse_deposit_and_delegate_shuttle_with_merge_accounts(
    accounts: &[AccountView],
) -> Result<DepositAndDelegateShuttleWithMergeAccounts<'_>, ProgramError> {
    let [payer_info, rent_pda_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, owner_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, _associated_token_program, system_program, destination_token_info, mint_info, token_program_info, global_vault_info, owner_source_token_info, vault_token_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    Ok(DepositAndDelegateShuttleWithMergeAccounts {
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
    })
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
