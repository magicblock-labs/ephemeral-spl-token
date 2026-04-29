use crate::processor::utils::GroupReceiptController;
use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;

use ephemeral_spl_api::state::group_receipt::TransferReceipt;
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, TransferQueueHeader, QUEUE_SEED,
};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, Address, ProgramResult};
use solana_signature::Signature;

pub const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
pub struct TransferCallbackArgs {
    /// Amount was transferred in action
    pub amount: u64,
    /// Group ID of a transfer
    pub group_id: u32,
    // Flags
    pub flag: u8,
}

pub fn derive_group_receipt_id(queue_address: &Address, group_id: u32) -> (Address, u8) {
    // TODO(edwin): maybe derive from sender too
    // Otherwise if group_ids circle to 1 there could be info leaks
    Address::find_program_address(
        &[
            GROUP_RECEIPT_SEED,
            queue_address.as_ref(),
            group_id.to_le_bytes().as_ref(),
        ],
        &crate::ID,
    )
}

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: [signer]             - Keypair : Validator authority
///  1: [writable]           - PDA     : Group receipt account.
///  2: [writable]           - PDA     : Transfer queue account.
///  3: []                   - SPL     : Vault account (unused).
///  4: []                   - SPL     : Mint account.
///  5: []                   - SPL     : Vault token account (unused).
///  6: []                   - Builtin : System program (unused).
///  7: []                   - SPL     : Token program (unused).
///  8: [writable]           - PDA     : Magic vault account.
///  9: []                   - Program : Magic program.
///
/// Instruction Data: MagicResponse
///
pub fn process_execute_transfer_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [validator, group_receipt, queue_info, _, mint, _, _, _, magic_vault, magic_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Verify validator & queue info
    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    validate_common(validator, queue_info, mint, header)?;

    let response = MagicResponseView::deserialize(instruction_data)?;
    let args = TransferCallbackArgs::decode(response.data)?;

    // Handles group receipt flow
    handle_group_receipt(
        queue_info,
        group_receipt,
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
    validator: &AccountView,
    queue_info: &AccountView,
    mint: &AccountView,
    queue_header: &TransferQueueHeader,
) -> ProgramResult {
    if !validator.is_signer() {
        #[cfg(feature = "logging")]
        pinocchio_log::log!("Missing authority to execute callback!");

        return Err(ProgramError::MissingRequiredSignature);
    }

    // Under condition that queue creation is authorized
    // Verifies both validator & queue
    let (derived_queue, _) = Address::find_program_address(
        &[
            QUEUE_SEED,
            mint.address().as_ref(),
            validator.address().as_ref(),
        ],
        &crate::ID,
    );
    if &derived_queue != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&crate::ID) {
        return Err(ProgramError::IllegalOwner);
    }

    // Verify mint
    if &queue_header.mint != mint.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

/// Handles group receipt flow
/// 1. Receipt doesn't exist - create
/// 2. Update receipt
/// 3. Closes if this is the last transfer
fn handle_group_receipt(
    queue_info: &AccountView,
    group_receipt_info: &AccountView,
    magic_vault: &AccountView,
    magic_program: &AccountView,
    args: &TransferCallbackArgsView<'_>,
    response: &MagicResponseView,
) -> ProgramResult {
    // Receipt specific validation
    let (group_receipt_id, group_receipt_bump) =
        derive_group_receipt_id(queue_info.address(), args.group_id());
    if &group_receipt_id != group_receipt_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    // Create receipt
    // This means that callback executed faster than initializing crank
    // As we don't know number of splits, initialize partially with 0
    let mut group_receipt = if !group_receipt_info.owned_by(&crate::ID) {
        #[cfg(feature = "logging")]
        pinocchio_log::log!("TransferCallback: initializing receipt");

        GroupReceiptController::create(
            group_receipt_info,
            queue_info,
            magic_vault,
            magic_program,
            group_receipt_bump,
            args.group_id(),
            0,
        )?
    } else {
        GroupReceiptController::view(group_receipt_info, queue_info, magic_vault, magic_program)?
    };
    group_receipt.record_transfer(TransferReceipt::new(
        response.signature.copied(),
        args.amount(),
        response.ok,
    ))?;

    // If no transfers left - close account
    if group_receipt.all_transfer_completed() {
        #[cfg(feature = "logging")]
        group_receipt.log();

        group_receipt.close()
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
