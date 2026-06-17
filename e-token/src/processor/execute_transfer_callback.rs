use crate::processor::internal::group_receipt;
use crate::processor::internal::group_receipt::{TransferCallbackArgs, TransferCallbackArgsView};
#[cfg(feature = "logging")]
use crate::processor::internal::group_receipt_log;
use crate::processor::internal::{group_receipt_close, GroupReceiptAccounts, CALLBACK_SIGNER};
use ephemeral_spl_api::state::group_receipt::{GroupReceipt, TransferReceipt};
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, TransferQueueHeader};
use ephemeral_spl_api::{
    debug_log, require, require_eq_keys, require_n_accounts, require_owned_by,
};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};
use solana_signature::Signature;
use wheels::layout::Decodable as _;

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
/// 10: []                   - Program : Magic program.
///
/// Instruction Data: MagicResponse
///
pub fn process_execute_transfer_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        callback_signer, // force multi-line
        group_receipt,
        queue_info,
        _vault,
        mint,
        _vault_token_account,
        source,
        _source_token_account,
        _token_program,
        magic_vault,
        magic_program,
    ] = require_n_accounts!(accounts, 11);

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

    #[cfg(feature = "logging")]
    if !response.ok {
        if let Ok(value) = core::str::from_utf8(response._error) {
            pinocchio_log::log!("Action failed: {}", value);
        }
    }

    // Handle transfer status
    // handle_transfer_status(..)?;

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

    let (expected_group_receipt, _) = group_receipt::derive_group_receipt_id(
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

/// Deserialize the bincode-encoded `MagicResponse` from a byte slice without
/// pulling in the `bincode` crate.
pub(crate) struct MagicResponseView<'a> {
    /// Whether the action completed successfully.
    pub ok: bool,
    /// Raw payload bytes the caller originally attached to the callback.
    pub data: &'a [u8],
    /// Error message bytes (empty when `ok` is true).
    pub _error: &'a [u8],
    /// 64-byte ed25519 signature of the action transaction, if available.
    pub signature: Option<&'a Signature>,
}

impl<'a> MagicResponseView<'a> {
    pub fn deserialize(src: &'a [u8]) -> Result<Self, ProgramError> {
        let mut cur = 0usize;

        // variant index – must be 0 (V1)
        // [0..4]  u32 LE  – enum variant index (must be 0 = V1)
        let variant = read_u32_le(src, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
        if variant != 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        // ok - action status
        // [4] ok (bool: 0 = false, non-zero = true)
        let ok = read_u8(src, &mut cur).ok_or(ProgramError::InvalidInstructionData)? != 0;

        // data payload
        // [5..13] u64 LE – payload byte length (N)
        let data_len =
            read_u64_le(src, &mut cur).ok_or(ProgramError::InvalidInstructionData)? as usize;
        // [13..13 + data_len] [u8; data_len] – payload bytes
        let data =
            read_slice(src, &mut cur, data_len).ok_or(ProgramError::InvalidInstructionData)?;

        // error string
        // [13 + data_len..21 + data_len] u64 LE – error string byte length
        let error_len =
            read_u64_le(src, &mut cur).ok_or(ProgramError::InvalidInstructionData)? as usize;
        // [21 + data_len, 21 + data_len + error_len]
        let error =
            read_slice(src, &mut cur, error_len).ok_or(ProgramError::InvalidInstructionData)?;

        // Option<ActionReceipt>
        // [21 + data_len + error_len] u8 - 1 byte tag for Option
        let tag = read_u8(src, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
        let signature: Option<&Signature> = match tag {
            0 => None,
            1 => {
                let bytes =
                    read_slice(src, &mut cur, 64).ok_or(ProgramError::InvalidInstructionData)?;
                // Safety: Signature is repr(transparent) over [u8; 64] and
                // read_slice guarantees exactly 64 bytes.
                Some(unsafe { &*(bytes.as_ptr() as *const Signature) })
            }
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        Ok(Self {
            ok,
            data,
            _error: error,
            signature,
        })
    }
}

#[inline(always)]
pub(crate) fn read_u8(src: &[u8], cur: &mut usize) -> Option<u8> {
    let val = *src.get(*cur)?;
    *cur += 1;
    Some(val)
}

#[inline(always)]
pub(crate) fn read_u32_le(src: &[u8], cur: &mut usize) -> Option<u32> {
    let end = cur.checked_add(4)?;
    if end > src.len() {
        return None;
    }
    let val = u32::from_le_bytes([src[*cur], src[*cur + 1], src[*cur + 2], src[*cur + 3]]);
    *cur = end;
    Some(val)
}

#[inline(always)]
pub(crate) fn read_u64_le(src: &[u8], cur: &mut usize) -> Option<u64> {
    let end = cur.checked_add(8)?;
    if end > src.len() {
        return None;
    }
    let val = u64::from_le_bytes([
        src[*cur],
        src[*cur + 1],
        src[*cur + 2],
        src[*cur + 3],
        src[*cur + 4],
        src[*cur + 5],
        src[*cur + 6],
        src[*cur + 7],
    ]);
    *cur = end;
    Some(val)
}

#[inline(always)]
pub(crate) fn read_slice<'a>(src: &'a [u8], cur: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = cur.checked_add(len)?;
    let slice = src.get(*cur..end)?;
    *cur = end;
    Some(slice)
}
