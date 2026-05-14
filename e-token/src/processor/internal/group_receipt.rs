use crate::processor::execute_transfer_callback::GROUP_RECEIPT_SEED;
use alloc::vec;
use alloc::vec::Vec;
use data_layout::{fixed_offset_layout, variable_offset_layout};
use ephemeral_spl_api::Address;
use pinocchio::error::ProgramError;
use solana_signature::Signature;

pub(crate) fn derive_group_receipt_id(
    queue_address: &Address,
    source: &Address,
    group_id: u32,
) -> (Address, u8) {
    Address::find_program_address(
        &[
            GROUP_RECEIPT_SEED,
            queue_address.as_ref(),
            source.as_ref(),
            &group_id.to_le_bytes(),
        ],
        &crate::ID,
    )
}

// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
pub(crate) struct TransferCallbackArgs {
    /// Amount was transferred in action
    pub amount: u64,
    /// Group ID of a transfer
    pub group_id: u32,
    // Flags
    pub flag: u8,
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
