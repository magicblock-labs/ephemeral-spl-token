#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::{
    args::{
        EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
        MaybeEncryptedPubkey, PostDelegationActions,
    },
    compact::{self, ClearTextWithInsertable},
};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::{
    consts, debug_log,
    instruction::ESplInstruction,
    instructions::{CloseStashArgs, DepositAndDelegateShuttleWithPrivateTransferArgs},
    require, require_eq_keys, require_n_accounts,
    state::{
        stash::StashPda,
        transfer_queue::{queue_views, TransferQueue},
    },
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
#[cfg(not(feature = "no-fees"))]
use solana_instruction::{AccountMeta, Instruction};
use wheels::layout::{Decodable as _, PrefixDecodable as _};

#[cfg(not(feature = "no-fees"))]
use crate::processor::internal::read_mint_decimals;
use crate::processor::internal::{
    get_associated_token_address,
    group_receipt::derive_group_receipt_id,
    shuttle_delegation::{
        merge_shuttle_into_token_account_action, process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
        undelegate_and_close_shuttle_action, DepositAndDelegateShuttleAccounts, DepositAndDelegateShuttleCommonArgs,
    },
    MAGIC_VAULT_ID,
};

#[cfg(not(feature = "no-fees"))]
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

/// Number of metas forwarded to scheduled instruction 25.
pub(crate) const SCHEDULED_PT_INNER_ACCOUNTS: usize = 19;

/// Total accounts on scheduled private transfer instructions (ix 25 layout + user ATA + Hydra crank).
pub(crate) const SCHEDULED_PT_ACCOUNTS: usize = SCHEDULED_PT_INNER_ACCOUNTS + 2;

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
/// Instruction Data: DepositAndDelegateShuttleWithPrivateTransferArgs,
/// optionally followed by `[user(32) | stash_bump(1)]` for scheduled
/// stash-close private transfers.
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
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
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
        queue_info,
    ] = require_n_accounts!(accounts, 19);

    let (args, remaining) = DepositAndDelegateShuttleWithPrivateTransferArgs::decode_prefix(instruction_data)?;

    require!(args.amount() != 0, ProgramError::InvalidInstructionData);

    let close_stash = Option::<CloseStashArgs>::decode(remaining)?;
    if let Some(close_stash_view) = close_stash.as_ref() {
        let derived_stash_pda = StashPda::derive_pda(
            close_stash_view.user(),
            mint_info.address(),
            close_stash_view.stash_bump(),
        )?;
        require_eq_keys!(payer_info.address(), &derived_stash_pda, ProgramError::InvalidSeeds);
    }

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
        let shuttle_ata = common_accounts.shuttle_wallet_ata_info.address().to_string();
        let mint = common_accounts.mint_info.address().to_string();
        let owner_source = common_accounts.owner_source_token_info.address().to_string();
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
    let derived_queue = TransferQueue::derive_pda(common_accounts.mint_info.address(), &queue_validator, bump)?;
    require_eq_keys!(&derived_queue, queue_info.address(), ProgramError::InvalidSeeds);

    let fee_amount = private_transfer_fee_amount(args.amount())?;
    let private_transfer_amount = if args.exact_out() {
        args.amount()
    } else {
        args.amount()
            .checked_sub(fee_amount)
            .ok_or(ProgramError::InvalidInstructionData)?
    };

    let total_amount = private_transfer_amount + fee_amount;

    let queue_vault_ata = get_associated_token_address(
        queue_info.address(),
        common_accounts.mint_info.address(),
        common_accounts.token_program_info.address(),
    );

    debug_log!(
        "exact_out:  {}, fee_amount: {}, private_transfer_amount: {}",
        args.exact_out(),
        fee_amount,
        private_transfer_amount
    );

    let actions = {
        let private_transfer = private_transfer_action_encrypted(
            &common_accounts,
            queue_info,
            &queue_vault_ata,
            args.encrypted_destination(),
            args.encrypted_data_suffix(),
            private_transfer_amount,
        )?;

        let mut post_actions = alloc::vec![merge_shuttle_into_token_account_action(
            &common_accounts,
            common_accounts.owner_source_token_info,
        )];

        #[cfg(not(feature = "no-fees"))]
        {
            let mint_decimals = read_mint_decimals(common_accounts.mint_info, common_accounts.token_program_info)?;
            post_actions.push(private_transfer_fee_action(
                &common_accounts,
                &queue_vault_ata,
                fee_amount,
                mint_decimals,
            ));
        }

        post_actions.push(undelegate_and_close_shuttle_action(&common_accounts, close_stash));
        post_actions.cleartext_with_insertable(private_transfer, 1)
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: args.shuttle_id(),
            total_amount,
            validator: args.validator(),
        },
        ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS,
        actions,
    )
}

fn private_transfer_action_encrypted(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    queue_vault_ata: &Address,
    encrypted_destination: &[u8; 80],
    encrypted_data_suffix: &[u8],
    amount: u64,
) -> Result<PostDelegationActions, ProgramError> {
    let group_id_raw = [
        encrypted_destination[0],
        encrypted_destination[1],
        encrypted_destination[2],
    ];
    let group_id = u32::from(group_id_raw[0]) | (u32::from(group_id_raw[1]) << 8) | (u32::from(group_id_raw[2]) << 16);
    let group_receipt_info =
        derive_group_receipt_id(queue_info.address(), common_accounts.owner_info.address(), group_id).0;
    Ok(PostDelegationActions {
        inserted_signers: 0,
        inserted_non_signers: 0,
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()],
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(crate::ID.to_bytes()),
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.owner_source_token_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(queue_vault_ata.to_bytes()),
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(encrypted_destination.into())),
            MaybeEncryptedPubkey::ClearText(common_accounts.token_program_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.shuttle_wallet_ata_info.address().to_bytes()),
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
                prefix: ESplInstruction::DepositAndQueueTransfer
                    .with_data(&[amount.to_le_bytes().as_slice(), group_id_raw.as_slice()].concat(),),
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
    queue_vault_ata: &Address,
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
            AccountMeta::new(*queue_vault_ata, false),
            AccountMeta::new_readonly(*common_accounts.owner_info.address(), true),
        ],
        data,
    }
}
