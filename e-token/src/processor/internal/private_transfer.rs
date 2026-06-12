#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::{
    args::{
        EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction,
        MaybeEncryptedIxData, MaybeEncryptedPubkey, PostDelegationActions,
    },
    compact::{self, ClearTextWithInsertable},
};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::{
    consts, debug_log,
    instruction::ESplInstruction,
    require, require_eq_keys, require_n_accounts,
    state::transfer_queue::{queue_views, TransferQueue},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_address::Address;
#[cfg(not(feature = "no-fees"))]
use solana_instruction::{AccountMeta, Instruction};

#[cfg(not(feature = "no-fees"))]
use crate::processor::internal::read_mint_decimals;
use crate::processor::internal::{
    get_associated_token_address,
    group_receipt::derive_group_receipt_id,
    shuttle_delegation::{
        merge_shuttle_into_token_account_action,
        process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
        undelegate_and_close_shuttle_action, CloseStashArgs, DepositAndDelegateShuttleAccounts,
        DepositAndDelegateShuttleCommonArgs,
    },
    MAGIC_VAULT_ID,
};

#[cfg(not(feature = "no-fees"))]
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

/// Number of metas forwarded to instruction 31.
pub(crate) const SCHEDULED_PT_INNER_ACCOUNTS: usize = 19;

/// Total accounts on scheduled private transfer instructions (ix 31 layout + user ATA + Hydra crank).
pub(crate) const SCHEDULED_PT_ACCOUNTS: usize = SCHEDULED_PT_INNER_ACCOUNTS + 2;

/// Shared body for ix 25 (`close_stash = None`) and ix 31 (`close_stash = Some`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_with_merge_and_private_transfer_inner(
    accounts: &[AccountView],
    shuttle_id: u32,
    amount: u64,
    exact_out: bool,
    validator: Option<&Address>,
    encrypted_destination: &[u8; 80],
    encrypted_data_suffix: &[u8],
    close_stash: Option<CloseStashArgs>,
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
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
        queue_info,
    ] = require_n_accounts!(accounts, 19);

    require!(amount != 0, ProgramError::InvalidInstructionData);

    let common_accounts = DepositAndDelegateShuttleAccounts {
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
    };

    require!(
        queue_info.owned_by(&ephemeral_spl_api::program::DELEGATION_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );

    debug_log!({
        let shuttle = common_accounts.shuttle_info.address().to_string();
        let shuttle_eata = common_accounts.shuttle_eata_info.address().to_string();
        let shuttle_ata = common_accounts
            .shuttle_wallet_ata_info
            .address()
            .to_string();
        let mint = common_accounts.mint_info.address().to_string();
        let owner_source = common_accounts
            .owner_source_token_info
            .address()
            .to_string();
        let vault_token = common_accounts.vault_token_info.address().to_string();
        let queue = queue_info.address().to_string();

        pinocchio_log::log!(
            350,
            "privatetx: shuttle={}, ata={} eata={} mint={} owner_source={} vault_token={} queue={}",
            shuttle.as_str(),
            shuttle_ata.as_str(),
            shuttle_eata.as_str(),
            mint.as_str(),
            owner_source.as_str(),
            vault_token.as_str(),
            queue.as_str(),
        );
    });

    let (bump, queue_validator) = {
        let (header, _) = queue_views(unsafe { queue_info.borrow_unchecked() })?;
        (header.bump, header.validator)
    };
    let derived_queue =
        TransferQueue::derive_pda(common_accounts.mint_info.address(), &queue_validator, bump)?;
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

    let fee_amount = private_transfer_fee_amount(amount)?;
    let private_transfer_amount = if exact_out {
        amount
    } else {
        amount
            .checked_sub(fee_amount)
            .ok_or(ProgramError::InvalidInstructionData)?
    };

    let total_amount = private_transfer_amount + fee_amount;

    debug_log!(
        "exact_out:  {}, fee_amount: {}, private_transfer_amount: {}",
        exact_out,
        fee_amount,
        private_transfer_amount
    );

    let actions = {
        let private_transfer = private_transfer_action_encrypted(
            &common_accounts,
            queue_info,
            encrypted_destination,
            encrypted_data_suffix,
            private_transfer_amount,
        )?;

        let mut post_actions = alloc::vec![merge_shuttle_into_token_account_action(
            &common_accounts,
            common_accounts.owner_source_token_info,
        )];

        #[cfg(not(feature = "no-fees"))]
        {
            let mint_decimals = read_mint_decimals(
                common_accounts.mint_info,
                common_accounts.token_program_info,
            )?;
            post_actions.push(private_transfer_fee_action(
                &common_accounts,
                fee_amount,
                mint_decimals,
            ));
        }

        post_actions.push(undelegate_and_close_shuttle_action(
            &common_accounts,
            close_stash,
        ));
        post_actions.cleartext_with_insertable(private_transfer, 1)
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id,
            total_amount,
            validator,
        },
        ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS,
        actions,
    )
}

fn private_transfer_action_encrypted(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    encrypted_destination: &[u8; 80],
    encrypted_data_suffix: &[u8],
    amount: u64,
) -> Result<PostDelegationActions, ProgramError> {
    let group_id_raw = [
        encrypted_destination[0],
        encrypted_destination[1],
        encrypted_destination[2],
    ];
    let group_id = u32::from(group_id_raw[0])
        | (u32::from(group_id_raw[1]) << 8)
        | (u32::from(group_id_raw[2]) << 16);
    let group_receipt_info = derive_group_receipt_id(
        queue_info.address(),
        common_accounts.owner_info.address(),
        group_id,
    )
    .0;
    let queue_vault_token_info = get_associated_token_address(
        queue_info.address(),
        common_accounts.mint_info.address(),
        common_accounts.token_program_info.address(),
    );
    Ok(PostDelegationActions {
        inserted_signers: 0,
        inserted_non_signers: 0,
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()],
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(crate::ID.to_bytes()),
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(
                common_accounts.owner_source_token_info.address().to_bytes()
            ),
            MaybeEncryptedPubkey::ClearText(queue_vault_token_info.to_bytes()),
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(encrypted_destination.into())),
            MaybeEncryptedPubkey::ClearText(
                common_accounts.token_program_info.address().to_bytes()
            ),
            MaybeEncryptedPubkey::ClearText(
                common_accounts.shuttle_wallet_ata_info.address().to_bytes()
            ),
            MaybeEncryptedPubkey::ClearText(group_receipt_info.to_bytes()),
            MaybeEncryptedPubkey::ClearText(MAGIC_VAULT_ID.to_bytes()),
            MaybeEncryptedPubkey::ClearText(MAGIC_PROGRAM_ID.to_bytes()),
        ],
        instructions: alloc::vec![MaybeEncryptedInstruction {
            program_id: 1,
            accounts: alloc::vec![
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(2, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(3, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(4, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(5, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(6, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(7, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(0, true)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(8, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(9, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(10, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(11, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(12, false)),
            ],
            data: MaybeEncryptedIxData {
                prefix: ESplInstruction::DepositAndQueueTransfer.with_data(
                    &[amount.to_le_bytes().as_slice(), group_id_raw.as_slice()].concat(),
                ),
                suffix: EncryptedBuffer::new(encrypted_data_suffix.into()),
            },
        }],
    })
}

#[inline(always)]
fn private_transfer_fee_amount(amount: u64) -> Result<u64, ProgramError> {
    Ok((amount as u128)
        .checked_mul(consts::PRIVATE_TRANSFER_FEE_BASIS_POINTS as u128)
        .ok_or(ProgramError::InvalidInstructionData)?
        .checked_div(consts::BASIS_POINTS_FACTOR)
        .ok_or(ProgramError::InvalidInstructionData)? as u64)
}

#[cfg(not(feature = "no-fees"))]
fn private_transfer_fee_action(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    fee_amount: u64,
    mint_decimals: u8,
) -> Instruction {
    let mut data = alloc::vec![TRANSFER_CHECKED_DISCRIMINATOR];
    data.extend_from_slice(&fee_amount.to_le_bytes());
    data.push(mint_decimals);

    Instruction {
        program_id: *common_accounts.token_program_info.address(),
        accounts: alloc::vec![
            AccountMeta::new(*common_accounts.owner_source_token_info.address(), false),
            AccountMeta::new_readonly(*common_accounts.mint_info.address(), false),
            AccountMeta::new(*common_accounts.vault_token_info.address(), false),
            AccountMeta::new_readonly(*common_accounts.owner_info.address(), true),
        ],
        data,
    }
}
