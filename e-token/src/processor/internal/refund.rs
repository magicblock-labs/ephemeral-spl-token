use ephemeral_rollups_pinocchio::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    intent_bundle::{ActionCallback, ShortAccountMeta},
    pda::magic_fee_vault_pda_from_validator,
};
use ephemeral_spl_api::{
    debug_log,
    error::EphemeralSplError,
    instructions::ExecuteQueuedTransferArgs,
    require, require_eq_keys, require_owned_by,
    state::transfer_queue::{queue_views_checked, QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_address::Address;
use wheels::{layout::Encodable as _, variable_offset_layout};

use crate::{
    instruction::ESplInternalInstruction,
    processor::internal::{
        queue_authorized_action::{
            invoke_standalone_action, IntentBundleAccounts, QueueSignerState, QueuedTransferActionBuilder,
            EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX,
        },
        token_program_for_kind, CALLBACK_SIGNER,
    },
};

pub(crate) const MAX_REFUND_RETRIES: u8 = 5;

pub(crate) struct RefundOnFailureAccounts<'a> {
    callback_signer: &'a AccountView,
    pub(crate) refund_destination_owner: &'a AccountView,
    pub(crate) queue_info: &'a AccountView,
    magic_fee_vault_info: &'a AccountView,
    magic_context_info: &'a AccountView,
    magic_program_info: &'a AccountView,
}

impl<'a> RefundOnFailureAccounts<'a> {
    pub(crate) fn try_new(
        callback_signer: &'a AccountView,
        refund_destination_owner: &'a AccountView,
        queue_info: &'a AccountView,
        magic_fee_vault_info: &'a AccountView,
        magic_context_info: &'a AccountView,
        magic_program_info: &'a AccountView,
    ) -> Result<Self, ProgramError> {
        if !callback_signer.is_signer() {
            debug_log!("Missing authority to execute callback!");
            return Err(ProgramError::MissingRequiredSignature);
        }
        require_eq_keys!(
            callback_signer.address(),
            &CALLBACK_SIGNER,
            ProgramError::IncorrectAuthority
        );
        require_eq_keys!(
            magic_context_info.address(),
            &MAGIC_CONTEXT_ID,
            ProgramError::InvalidSeeds
        );
        require_eq_keys!(
            magic_program_info.address(),
            &MAGIC_PROGRAM_ID,
            ProgramError::InvalidSeeds
        );
        require_owned_by!(queue_info, &crate::ID);

        Ok(Self {
            callback_signer,
            refund_destination_owner,
            queue_info,
            magic_fee_vault_info,
            magic_context_info,
            magic_program_info,
        })
    }
}

pub(crate) fn schedule_refund_on_failure(
    accounts: &RefundOnFailureAccounts<'_>,
    amount: u64,
    retries_left: u8,
) -> ProgramResult {
    if retries_left == 0 {
        log_refund_permanently_failed(accounts, amount);
        return Err(EphemeralSplError::RefundPermanentlyFailed.into());
    }

    let RefundOnFailureAccounts {
        callback_signer,
        refund_destination_owner,
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    } = accounts;

    let queue_data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(queue_data)?;

    let derived_magic_fee_vault = magic_fee_vault_pda_from_validator(&header.validator);
    require!(
        derived_magic_fee_vault.to_bytes() == magic_fee_vault_info.address().to_bytes(),
        ProgramError::InvalidSeeds
    );

    let mint = header.mint;
    let token_program = token_program_for_kind(header.token_program_kind()?);
    let (vault, _) = Address::find_program_address(&[mint.as_ref()], &crate::ID);

    let callback_accounts = create_callback_accounts(
        callback_signer.address(),
        refund_destination_owner.address(),
        queue_info.address(),
        magic_fee_vault_info.address(),
    );
    let encoded_refund_args = RefundOnFailureArgs {
        amount,
        retries_left: retries_left - 1,
    }
    .encode()?;
    let callback = create_callback(&callback_accounts, &encoded_refund_args);

    let action_builder = QueuedTransferActionBuilder::new(
        queue_info,
        refund_destination_owner.address(),
        &vault,
        &mint,
        &token_program,
        ExecuteQueuedTransferArgs {
            amount,
            // TODO(edwin): clarify if needed
            client_ref_id: None,
            escrow_index: EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX,
            flags: QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
        },
    );
    let mut refund_action = action_builder.build();
    refund_action.callback = Some(callback);

    invoke_standalone_action(
        &IntentBundleAccounts {
            queue_info,
            magic_program_info,
            magic_context_info,
            magic_fee_vault_info,
        },
        &QueueSignerState {
            mint: header.mint,
            queue_bump: header.bump,
            validator: header.validator,
        },
        &[refund_action],
    )
}

// buffer_offset = 6: response.data starts at byte 14 of the original 8-byte-aligned
// instruction buffer (1 disc + 4 variant + 1 ok + 8 data_len), and 14 % 8 = 6.
#[variable_offset_layout(buffer_offset = 6)]
pub(crate) struct RefundOnFailureArgs {
    /// Amount to be refunded
    pub amount: u64,
    /// Remaining retry attempts; when 0 the callback gives up
    pub retries_left: u8,
}

fn create_callback_accounts(
    callback_signer: &Address,
    refund_destination_owner: &Address,
    queue: &Address,
    magic_fee_vault: &Address,
) -> [ShortAccountMeta; 6] {
    [
        ShortAccountMeta {
            pubkey: callback_signer.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: refund_destination_owner.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: queue.clone(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: magic_fee_vault.clone(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: MAGIC_CONTEXT_ID,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: MAGIC_PROGRAM_ID,
            is_writable: false,
        },
    ]
}

fn create_callback<'a>(accounts: &'a [ShortAccountMeta], payload: &'a [u8]) -> ActionCallback<'a> {
    const CALLBACK_COMPUTE_UNITS: u32 = 100_000;

    ActionCallback {
        destination_program: crate::ID,
        discriminator: &[ESplInternalInstruction::RefundOnFailureCallback as u8],
        payload,
        compute_units: CALLBACK_COMPUTE_UNITS,
        accounts,
    }
}

#[cfg_attr(not(feature = "logging"), allow(unused_variables))]
fn log_refund_permanently_failed(accounts: &RefundOnFailureAccounts<'_>, amount: u64) {
    #[cfg(feature = "logging")]
    {
        use alloc::string::ToString;
        pinocchio_log::log!(
            "Refund permanently failed: amount={} destination={} queue={}",
            amount,
            accounts.refund_destination_owner.address().to_string().as_str(),
            accounts.queue_info.address().to_string().as_str(),
        );
    }
}
