use ephemeral_spl_api::{
    debug_log, require, require_eq_keys, require_n_accounts, require_owned_by,
    state::{
        group_receipt::{GroupReceipt, TransferReceipt},
        transfer_queue::{queue_views_checked, TransferQueueHeader},
    },
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use wheels::layout::Decodable as _;

#[cfg(feature = "logging")]
use crate::processor::internal::group_receipt_log;
use crate::processor::internal::{
    callbacks::MagicResponseView,
    group_receipt::{derive_group_receipt_id, TransferCallbackArgs, TransferCallbackArgsView},
    group_receipt_close,
    refund::{schedule_refund_on_failure, RefundOnFailureAccounts, MAX_REFUND_RETRIES},
    GroupReceiptAccounts, CALLBACK_SIGNER,
};

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
/// Accounts (11-account shape, legacy — no refund on failure):
///
///  0-9: same as above.
/// 10: []                   - Program : Magic program.
///
/// Instruction Data: MagicResponse
///
pub fn process_execute_transfer_callback(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    // TODO(edwin/snawaz): remove 11-account compat path after next deployment
    let (callback_signer, group_receipt, queue_info, mint, source, magic_vault, magic_program, maybe_refund) =
        if accounts.len() >= 13 {
            let [cb, gr, qi, _vault, mint, _vault_ta, src, _src_ta, _tp, mv, mfv, mp, mc] =
                require_n_accounts!(accounts, 13);
            (cb, gr, qi, mint, src, mv, mp, Some((mfv, mc)))
        } else {
            let [cb, gr, qi, _vault, mint, _vault_ta, src, _src_ta, _tp, mv, mp] = require_n_accounts!(accounts, 11);
            (cb, gr, qi, mint, src, mv, mp, None)
        };

    // Verify validator & queue info
    let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
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

        if let Some((magic_fee_vault, magic_context)) = maybe_refund {
            return schedule_refund_on_failure(
                &RefundOnFailureAccounts::try_new(
                    callback_signer,
                    source,
                    queue_info,
                    magic_fee_vault,
                    magic_context,
                    magic_program,
                )?,
                args.amount(),
                MAX_REFUND_RETRIES,
            );
        }
    }

    Ok(())
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
    require_eq_keys!(&queue_header.mint, mint.address(), ProgramError::InvalidAccountData);

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
    debug_log!({
        use alloc::string::ToString;
        pinocchio_log::log!(
            256,
            "ExecuteTransferCallback group_receipt address: {} data_len: {} owner: {}",
            group_receipt_info.address().to_string().as_str(),
            group_receipt_info.data_len(),
            unsafe { group_receipt_info.owner() }.to_string().as_str()
        );
    });

    if !group_receipt_info.owned_by(&crate::ID) {
        debug_log!("Group receipt expected to be initialized");
        return Err(ProgramError::InvalidAccountOwner);
    }

    let mut group_receipt = GroupReceipt::new(group_receipt_info)?;
    if group_receipt.id() != args.group_id() {
        debug_log!("Callback with wrong group id");
        return Err(ProgramError::InvalidArgument);
    }

    let (expected_group_receipt, _) =
        derive_group_receipt_id(queue_info.address(), source.address(), group_receipt.id());
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
