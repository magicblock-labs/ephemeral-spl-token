use crate::processor::process_transfer_queue_tick::derive_associated_token_address;
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

pub fn process_execute_transfer_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [validator, vault, mint, vault_token_account, source_token_account, token_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !validator.is_signer() {
        pinocchio_log::log!("Missing authority to execute callback!");
        return Err(ProgramError::MissingRequiredSignature);
    }
    // TODO(edwin): validator address validation

    let derived_vault_token = derive_associated_token_address(vault.address(), mint.address());
    if vault_token_account.address() != &derived_vault_token {
        pinocchio_log::log!("invalid vault token address");
        return Err(ProgramError::InvalidAccountData);
    }

    let response = MagicResponseView::deserialize(instruction_data)?;
    if response.ok {
        pinocchio_log::log!("Action succeeded!");
    } else {
        pinocchio_log::log!("Action failed!");
    }

    let mut cur = 0;
    let amount =
        read_u64_le(response.data, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
    let flag = read_u8(response.data, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
    pinocchio_log::log!("callback amount: {}", amount);
    pinocchio_log::log!("callback flags: {}", flag);

    Ok(())
}

/// Deserialize the bincode-encoded `MagicResponse` from a byte slice without
/// pulling in the `bincode` crate.
pub(crate) struct MagicResponseView<'a> {
    /// Whether the action completed successfully.
    pub ok: bool,
    /// Raw payload bytes the caller originally attached to the callback.
    pub data: &'a [u8],
    /// Error message bytes (empty when `ok` is true).
    pub error: &'a [u8],
    /// 64-byte ed25519 signature of the action transaction, if available.
    pub signature: Option<&'a [u8; 64]>,
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
        let signature: Option<&[u8; 64]> = match tag {
            0 => None,
            1 => {
                let bytes =
                    read_slice(src, &mut cur, 64).ok_or(ProgramError::InvalidInstructionData)?;
                // Safety: read_slice guarantees exactly 64 bytes.
                Some(
                    bytes
                        .try_into()
                        .map_err(|_| ProgramError::InvalidInstructionData)?,
                )
            }
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        Ok(Self {
            ok,
            data,
            error,
            signature,
        })
    }
}

#[inline(always)]
fn read_u8(src: &[u8], cur: &mut usize) -> Option<u8> {
    let val = *src.get(*cur)?;
    *cur += 1;
    Some(val)
}

#[inline(always)]
fn read_u32_le(src: &[u8], cur: &mut usize) -> Option<u32> {
    let end = cur.checked_add(4)?;
    if end > src.len() {
        return None;
    }
    let val = u32::from_le_bytes([src[*cur], src[*cur + 1], src[*cur + 2], src[*cur + 3]]);
    *cur = end;
    Some(val)
}

#[inline(always)]
fn read_u64_le(src: &[u8], cur: &mut usize) -> Option<u64> {
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
fn read_slice<'a>(src: &'a [u8], cur: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = cur.checked_add(len)?;
    let slice = src.get(*cur..end)?;
    *cur = end;
    Some(slice)
}
