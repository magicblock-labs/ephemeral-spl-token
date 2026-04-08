use crate::processor::ephemeral_account::{close_ephemeral_account, create_ephemeral_account};
use crate::processor::process_transfer_queue_tick::derive_associated_token_address;
use ephemeral_spl_api::program::id_address;
use ephemeral_spl_api::state::group_receipt::{
    initialize_group_receipt, GroupReceipt, TransferReceipt,
};
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, TransferQueueHeader, QUEUE_SEED,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, Address, ProgramResult};
use solana_signature::Signature;

pub const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

pub struct TransferCallbackArgs {
    /// Amount was transferred in action
    amount: u64,
    /// Group ID of a transfer
    group_id: u32,
    /// Number of splits in group
    splits: u32,
    // Flags
    _flag: u8,
}

impl TransferCallbackArgs {
    pub fn try_from_bytes(data: &[u8]) -> Result<Self, ProgramError> {
        let mut cur = 0;
        let amount = read_u64_le(data, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;
        let group_id = read_u32_le(data, &mut cur).ok_or(ProgramError::InvalidAccountData)?;
        let splits = read_u32_le(data, &mut cur).ok_or(ProgramError::InvalidAccountData)?;
        let flag = read_u8(data, &mut cur).ok_or(ProgramError::InvalidInstructionData)?;

        Ok(Self {
            amount,
            group_id,
            splits,
            _flag: flag,
        })
    }
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
        &id_address(),
    )
}

pub fn process_execute_transfer_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [validator, group_receipt, queue_info, vault, mint, vault_token_account, _, _, magic_vault] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Verify validator & queue info
    let data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(data)?;
    validate(validator, queue_info, mint, header)?;

    // TODO(edwin): join with above?
    // above can be renamed into validate_queue
    // Verify vault
    let derived_vault_token = derive_associated_token_address(vault.address(), mint.address());
    if vault_token_account.address() != &derived_vault_token {
        pinocchio_log::log!("Invalid vault token address");
        return Err(ProgramError::InvalidSeeds);
    }

    let response = MagicResponseView::deserialize(instruction_data)?;
    let args = TransferCallbackArgs::try_from_bytes(response.data)?;

    // Handles group receipt flow
    handle_group_receipt(queue_info, group_receipt, magic_vault, &args, &response)?;
    if !response.ok {
        if let Ok(value) = core::str::from_utf8(response.error) {
            pinocchio_log::log!("Action failed: {}", value);
        }
    }

    // Handle transfer status
    // handle_transfer_status(..)?;

    Ok(())
}

fn validate(
    validator: &AccountView,
    queue_info: &AccountView,
    mint: &AccountView,
    queue_header: &TransferQueueHeader,
) -> ProgramResult {
    if !validator.is_signer() {
        pinocchio_log::log!("Missing authority to execute callback!");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // TODO(edwin): verify
    // Under condition that queue can be created only by validator
    // Verifies both validator & queue
    let (derived_queue, _) = Address::find_program_address(
        &[
            QUEUE_SEED,
            mint.address().as_ref(),
            validator.address().as_ref(),
        ],
        &id_address(),
    );
    if &derived_queue != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&id_address()) {
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
    args: &TransferCallbackArgs,
    response: &MagicResponseView,
) -> ProgramResult {
    let (group_receipt_id, group_receipt_bump) =
        derive_group_receipt_id(queue_info.address(), args.group_id);
    if &group_receipt_id != group_receipt_info.address() {
        pinocchio_log::log!("Invalid group receipt account");
        // TODO(edwin): should error?
        return Err(ProgramError::InvalidSeeds);
    }

    // Create receipt idempotently
    init_group_receipt_id(
        queue_info,
        group_receipt_info,
        magic_vault,
        group_receipt_bump,
        args,
    )?;
    // Update receipt recording transfer
    let mut group_receipt = GroupReceipt::new(group_receipt_info)?;
    group_receipt.record_transfer(TransferReceipt::new(
        response.signature.copied(),
        args.amount,
        response.ok,
    ))?;

    // If no transfers left - close account
    if group_receipt.transfers_left() == 0 {
        log_group_receipt(&group_receipt);
        close_group_receipt(queue_info, group_receipt_info, magic_vault)
    } else {
        Ok(())
    }
}

#[inline(never)]
fn log_group_receipt(group_receipt: &GroupReceipt) {
    pinocchio_log::log!("All transfers complete for group id:{}", group_receipt.id());
    if let Ok(items) = group_receipt.items() {
        for (i, item) in items.iter().enumerate() {
            match item.signature() {
                Some(sig) => pinocchio_log::log!(
                    "transfer[{}] ok:{} amount:{} sig:{}",
                    i as u32,
                    item.ok(),
                    item.amount(),
                    sig.as_ref()
                ),
                None => pinocchio_log::log!(
                    "transfer[{}] ok:{} amount:{} sig:None",
                    i as u32,
                    item.ok(),
                    item.amount()
                ),
            }
        }
    }
}

/// Returns a reference to the `GroupReceipt`
///
/// If the PDA account already exists (owned by this program) the stored data
/// is returned directly.
/// Otherwise, the account is created via CPI, funded from `queue_info`.
#[inline(never)]
pub fn init_group_receipt_id(
    queue_info: &AccountView,
    group_receipt: &AccountView,
    magic_vault: &AccountView,
    group_receipt_bump: u8,
    callback_args: &TransferCallbackArgs,
) -> ProgramResult {
    // Account already exists — nothing to do.
    if group_receipt.owned_by(&id_address()) {
        return Ok(());
    }

    // Build queue signer seeds from its stored header.
    let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let queue_signer = Signer::from(&queue_signer_seeds);

    let group_id_bytes = callback_args.group_id.to_le_bytes();
    let receipt_bump_seed = [group_receipt_bump];
    let receipt_signer_seeds = [
        Seed::from(GROUP_RECEIPT_SEED),
        Seed::from(queue_info.address().as_ref()),
        Seed::from(group_id_bytes.as_ref()),
        Seed::from(&receipt_bump_seed),
    ];
    let receipt_signer = Signer::from(&receipt_signer_seeds);

    // Account does not exist yet — create it as an ephemeral account, paying from the queue PDA.
    let space = GroupReceipt::required_size(callback_args.splits as usize);
    create_ephemeral_account(
        queue_info,
        group_receipt,
        magic_vault,
        space as u32,
        &[queue_signer, receipt_signer],
    )?;

    // Write initial state into the newly allocated account.
    initialize_group_receipt(
        group_receipt,
        callback_args.group_id,
        callback_args.splits,
        group_receipt_bump,
    )
}

pub fn close_group_receipt(
    queue_info: &AccountView,
    group_receipt: &AccountView,
    magic_vault: &AccountView,
) -> ProgramResult {
    let (header, _) = queue_views_checked(unsafe { queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let queue_signer = Signer::from(&queue_signer_seeds);

    close_ephemeral_account(queue_info, group_receipt, magic_vault, &[queue_signer])
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
