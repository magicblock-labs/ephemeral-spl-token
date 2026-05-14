use crate::processor::internal::callbacks::{
    MagicResponseView, TransferCallbackArgs, TransferCallbackArgsView,
};
use crate::processor::internal::group_receipt_accounts;
#[cfg(feature = "logging")]
use crate::processor::internal::group_receipt_accounts::group_receipt_log;
use crate::processor::internal::group_receipt_accounts::{
    group_receipt_close, GroupReceiptAccounts,
};
use crate::processor::refund_on_failure_callback::{
    schedule_refund_on_failure, RefundOnFailureAccounts,
};
use crate::processor::utils::CALLBACK_SIGNER;
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, TransferReceipt};
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, TransferQueueHeader};
use ephemeral_spl_api::{
    debug_log, require, require_eq_keys, require_n_accounts, require_owned_by,
};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

pub const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: [signer]             - PDA     : Callback signer (CALLBACK_SIGNER).
///  1: [writable]           - PDA     : Group receipt account.
///  2: [writable]           - PDA     : Transfer queue account.
///  3: []                   - PDA     : Vault account (unused).
///  4: []                   - SPL     : Mint account.
///  5: []                   - SPL     : Vault token account (unused).
///  6: []                   - Any     : Source owner account.
///  7: []                   - SPL     : Source token account (unused).
///  8: []                   - Builtin : Token program (unused).
///  9: [writable]           - PDA     : Magic vault account.
/// 10: [writable]           - PDA     : Magic fee vault account.
/// 11: []                   - Program : Magic program.
/// 12: [writable]           - PDA     : Magic context account.
///
/// Instruction Data: MagicResponse
///
pub fn process_execute_transfer_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        callback_signer,
        group_receipt,
        queue_info,
        _, // vault
        mint,
        _, // vault token account
        source,
        _, // source token account
        _, // token program
        magic_vault,
        magic_fee_vault,
        magic_program,
        magic_context,
    ] = require_n_accounts!(accounts, 13);

    // Verify validator & queue info
    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    validate_common(callback_signer, queue_info, mint, header)?;

    let response = MagicResponseView::deserialize(instruction_data)?;
    let args = TransferCallbackArgs::decode(response.data)?;

    // Handles group receipt flow
    handle_group_receipt(
        queue_info,
        group_receipt,
        source,
        magic_vault,
        magic_program,
        &args,
        &response,
    )?;

    // Handles refunds
    if !response.ok {
        #[cfg(feature = "logging")]
        {
            use alloc::string::ToString;
            if let Some(signature) = response.signature {
                pinocchio_log::log!(
                    "Failed to transfer: {}, signature: {}",
                    args.amount(),
                    signature.to_string().as_str()
                );
            } else {
                pinocchio_log::log!("Failed to transfer: {}", args.amount());
            }
        }

        schedule_refund_on_failure(
            &RefundOnFailureAccounts::try_new(
                callback_signer,
                source,
                queue_info,
                magic_fee_vault,
                magic_context,
                magic_program,
            )?,
            args.amount(),
        )
    } else {
        Ok(())
    }
}

fn validate_common(
    callback_signer: &AccountView,
    queue_info: &AccountView,
    mint: &AccountView,
    queue_header: &TransferQueueHeader,
) -> ProgramResult {
    if !callback_signer.is_signer() {
        debug_log!("Missing authority to execute callback!");
        return Err(ProgramError::MissingRequiredSignature);
    }

    require_eq_keys!(
        callback_signer.address(),
        &CALLBACK_SIGNER,
        ProgramError::IncorrectAuthority
    );
    require_owned_by!(queue_info, &crate::ID);
    require_eq_keys!(
        &queue_header.mint,
        mint.address(),
        ProgramError::InvalidAccountData
    );

    Ok(())
}

/// Handles group receipt flow
/// 1. Receipt doesn't exist - create
/// 2. Update receipt
/// 3. Closes if this is the last transfer
fn handle_group_receipt(
    queue_info: &AccountView,
    group_receipt_info: &AccountView,
    source: &AccountView,
    magic_vault: &AccountView,
    magic_program: &AccountView,
    args: &TransferCallbackArgsView<'_>,
    response: &MagicResponseView,
) -> ProgramResult {
    if !group_receipt_info.owned_by(&crate::ID) {
        debug_log!("Group receipt expected to be initialized");
        return Err(ProgramError::InvalidAccountOwner);
    }

    let mut group_receipt = GroupReceipt::new(group_receipt_info)?;
    if group_receipt.id() != args.group_id() {
        debug_log!("Callback with wrong group id");
        return Err(ProgramError::InvalidArgument);
    }

    let (expected_group_receipt, _) = group_receipt_accounts::derive_group_receipt_id(
        queue_info.address(),
        source.address(),
        group_receipt.id(),
    );
    require!(
        expected_group_receipt.eq(group_receipt_info.address()),
        ProgramError::InvalidAccountData
    );

    group_receipt
        .record_transfer(TransferReceipt::new(
            response.signature.copied(),
            args.amount(),
            response.ok,
        ))
        .map_err(|_| ProgramError::InvalidInstructionData)?;

    // If no transfers left - close account
    if group_receipt.all_transfer_completed() {
        #[cfg(feature = "logging")]
        group_receipt_log(&group_receipt);

        group_receipt_close(
            &GroupReceiptAccounts {
                group_receipt_info,
                queue_info,
                source,
                magic_vault,
                _magic_program: magic_program,
            },
            group_receipt,
        )
    } else {
        Ok(())
    }
}
