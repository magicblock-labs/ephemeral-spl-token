use core::u64;

#[cfg(feature = "logging")]
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;

use dlp_api::args::{
    EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
    MaybeEncryptedPubkey,
};
use dlp_api::compact::{self};

use ephemeral_spl_api::debug_log;
use ephemeral_spl_api::instruction::ESplInstruction;
use ephemeral_spl_api::state::transfer_queue::{queue_views, TransferQueue};
use ephemeral_spl_api::{consts, require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
#[cfg(not(feature = "no-fees"))]
use solana_instruction::{AccountMeta, Instruction};

use crate::processor::internal::group_receipt::derive_group_receipt_id;
use crate::processor::internal::shuttle_delegation::{
    merge_shuttle_into_token_account_action,
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
    undelegate_and_close_shuttle_action, CloseStashArgs, DepositAndDelegateShuttleAccounts,
    DepositAndDelegateShuttleCommonArgs,
};
#[cfg(not(feature = "no-fees"))]
use crate::processor::utils::read_mint_decimals;
use crate::processor::utils::{get_associated_token_address, MAGIC_VAULT_ID};
use dlp_api::{args::PostDelegationActions, compact::ClearTextWithInsertable};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;

#[cfg(not(feature = "no-fees"))]
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

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

/// Shared body for ix 25 (`close_stash = None`) and ix 31 (`close_stash = Some`).
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_with_merge_and_private_transfer_inner(
    accounts: &[AccountView],
    shuttle_id: u32,
    amount: u64,
    exact_out: bool,
    validator: Option<&[u8; 32]>,
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

    // require queue_info to already be delegated to the delegated program.
    require!(
        queue_info.owned_by(&ephemeral_spl_api::program::DELEGATION_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );

    // CHECKPOINT: this entire log wont be printed because of message size (see logs on explorer)
    debug_log!({
        let shuttle = common_accounts.shuttle_info.address().to_string();
        let shuttle_eata = common_accounts.shuttle_eata_info.address().to_string();
        let shuttle_wallet = common_accounts
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
            "Private shuttle ix accounts shuttle={} shuttle_eata={} shuttle_wallet={} mint={} owner_source={} vault_token={} queue={}",
            shuttle.as_str(),
            shuttle_eata.as_str(),
            shuttle_wallet.as_str(),
            mint.as_str(),
            owner_source.as_str(),
            vault_token.as_str(),
            queue.as_str(),
        );
    });

    let (bump, queue_validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views(data)?;
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

#[variable_offset_layout(buffer_offset = 1)]
pub struct DepositAndDelegateShuttleWithPrivateTransferArgs {
    pub shuttle_id: u32,
    //
    // The interpretation of amount field depends on the value of exact_out.
    //
    // - If exact_out == true:
    //   Then amount is amount_out, the exact amount received by the recipient and
    //   fees are deducted from the sender.
    //
    // - If exact_out == false:
    //   Then amount is amount_in, the exact amount deducted from the sender and
    //   the recipient_amount = amount - fee.
    //
    pub amount: u64,
    pub exact_out: bool,
    //
    // [capacity = 80] is because sealed-box encryption adds 48 bytes of overhead
    // irrespective of input bytes. So since encrypted_destination is encrypted
    // pubkey, its len has to be exactly: 32 + 48 = 80
    //
    // ref: https://github.com/jedisct1/libsodium-rs/blob/b3ad9336c0/src/crypto_box/mod.rs#L229-L232
    //
    pub encrypted_destination: [u8; 80],
    pub validator: Option<[u8; 32]>,
    //
    // This becomes the encrypted suffix of instruction-data for DepositAndQueueTransfer. So this
    // suffix has everthing of DepositAndQueueTransferArgs except the first field (amount: u64)
    //
    // min_delay_ms: u64,
    // max_delay_ms: u64,
    // split: u32,
    // flags: Option<u8>,
    // client_ref_id: Option<u64>,
    //
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
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
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()], // 0
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(crate::ID.to_bytes()), // 1
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()), // 2
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()), // 3
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()), // 4
            MaybeEncryptedPubkey::ClearText(
                common_accounts.owner_source_token_info.address().to_bytes()
            ), // 5
            MaybeEncryptedPubkey::ClearText(queue_vault_token_info.to_bytes()), // 6
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(encrypted_destination.into())), // 7
            MaybeEncryptedPubkey::ClearText(
                common_accounts.token_program_info.address().to_bytes()
            ), // 8
            MaybeEncryptedPubkey::ClearText(
                common_accounts.shuttle_wallet_ata_info.address().to_bytes()
            ), // 9
            MaybeEncryptedPubkey::ClearText(group_receipt_info.to_bytes()), // 10
            MaybeEncryptedPubkey::ClearText(MAGIC_VAULT_ID.to_bytes(),),    // 11
            MaybeEncryptedPubkey::ClearText(MAGIC_PROGRAM_ID.to_bytes(),),  // 12
        ],
        instructions: alloc::vec![MaybeEncryptedInstruction {
            program_id: 1,
            accounts: alloc::vec![
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(2, false)), // queue_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(3, false)), // queue vault authority
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(4, false)), // mint_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(5, false)), // owner_source_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(6, false)), // vault_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(7, false)), // destination_owner_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(0, true)), // owner_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(8, false)), // token_program_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(9, false)), // shuttle_wallet_ata_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(10, false)), // group_receipt
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(11, false)), // magic_vault
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(12, false)), // magic_program
            ],
            data: MaybeEncryptedIxData {
                prefix: ESplInstruction::DepositAndQueueTransfer.with_data(
                    &[amount.to_le_bytes().as_slice(), group_id_raw.as_slice()].concat()
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
