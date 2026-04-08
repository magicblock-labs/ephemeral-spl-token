use crate::processor::process_transfer_queue_tick::derive_associated_token_address;
use ephemeral_spl_api::program::id_address;
use ephemeral_spl_api::state::group_receipt::GroupReceiptHeader;
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, TransferQueueHeader, QUEUE_SEED,
};
use ephemeral_spl_api::state::{load_initialized, load_mut, load_mut_initialized, RawType};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{AccountView, Address, ProgramResult};
use pinocchio_system::instructions::CreateAccount;
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
    flag: u8,
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
            flag,
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
    let [validator, group_receipt, queue_info, vault, mint, vault_token_account, source_token_account, token_program] =
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

    // #[cfg(feature = "logging")]
    match response.signature {
        Some(sig) => pinocchio_log::log!(
            "TransferCallback group_id: {}, status: {}, signature: {}",
            args.group_id,
            response.ok,
            sig.as_ref(),
        ),
        None => pinocchio_log::log!(
            "TransferCallback group_id: {}, status: {}, signature: None",
            args.group_id,
            response.ok,
        ),
    }

    if response.ok {
        pinocchio_log::log!("Action succeeded!");
    } else {
        pinocchio_log::log!("Action failed!");
    }

    // Handles group receipt flow
    handle_group_receipt(queue_info, group_receipt, &args)?;
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
    let (derived_queue, queue_bump) = Address::find_program_address(
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
/// Receipt doesn't exist - create
/// Update receipt
/// Closes if this is the last transfer
fn handle_group_receipt(
    queue_info: &AccountView,
    group_receipt_info: &AccountView,
    args: &TransferCallbackArgs,
) -> ProgramResult {
    let (group_receipt_id, group_receipt_bump) =
        derive_group_receipt_id(queue_info.address(), args.group_id);
    if &group_receipt_id != group_receipt_info.address() {
        pinocchio_log::log!("Invalid group receipt account");
        // TODO(edwin): should error?
        return Err(ProgramError::InvalidSeeds);
    }

    init_group_receipt_id(queue_info, group_receipt_info, group_receipt_bump, args)?;

    let group_receipt = load_mut_initialized::<GroupReceiptHeader>(unsafe {
        group_receipt_info.borrow_unchecked_mut()
    })?;
    group_receipt.transfer_complete();
    if group_receipt.transfers_left() == 0 {
        let _ = group_receipt;
        close_group_receipt(queue_info, group_receipt_info)
    } else {
        Ok(())
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
    group_receipt_bump: u8,
    callback_args: &TransferCallbackArgs,
) -> ProgramResult {
    // Account already exists — nothing to do.
    if group_receipt.owned_by(&id_address()) {
        return Ok(());
    }

    // Account does not exist yet — create it, paying from the queue PDA.
    let lamports = Rent::get()?.try_minimum_balance(GroupReceiptHeader::LEN)?;

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

    CreateAccount {
        from: queue_info,
        to: group_receipt,
        space: GroupReceiptHeader::LEN as u64,
        lamports,
        owner: &id_address(),
    }
    .invoke_signed(&[queue_signer, receipt_signer])?;

    // Write initial state into the newly allocated account.
    let data = unsafe { group_receipt.borrow_unchecked_mut() };
    let receipt = load_mut::<GroupReceiptHeader>(data)?;
    *receipt = GroupReceiptHeader::new(
        callback_args.group_id,
        group_receipt_bump,
        callback_args.splits,
    );

    Ok(())
}

pub fn close_group_receipt(payer: &AccountView, group_receipt: &AccountView) -> ProgramResult {
    todo!()
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
