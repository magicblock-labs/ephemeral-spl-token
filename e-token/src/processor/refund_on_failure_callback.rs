use crate::instruction::ESplInternalInstruction;
use crate::processor::internal::group_receipt::MagicResponseView;
use crate::processor::internal::queue_authorized_action::{
    invoke_standalone_action, IntentBundleAccounts, QueueSignerState, QueuedTransferActionBuilder,
};
use crate::processor::utils::CALLBACK_SIGNER;
use crate::ExecuteQueuedTransferArgs;
use data_layout::fixed_offset_layout;
use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_rollups_pinocchio::intent_bundle::{ActionCallback, ShortAccountMeta};
use ephemeral_rollups_pinocchio::pda::magic_fee_vault_pda_from_validator;
use ephemeral_spl_api::state::transfer_queue::{
    queue_views_checked, QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
};
use ephemeral_spl_api::{
    debug_log, require, require_eq_keys, require_n_accounts, require_owned_by,
};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;

pub(crate) struct RefundOnFailureAccounts<'a> {
    pub(crate) callback_signer: &'a AccountView,
    pub(crate) refund_destination_owner: &'a AccountView,
    pub(crate) queue_info: &'a AccountView,
    pub(crate) magic_fee_vault_info: &'a AccountView,
    pub(crate) magic_context_info: &'a AccountView,
    pub(crate) magic_program_info: &'a AccountView,
}

impl RefundOnFailureAccounts<'_> {
    fn validate(&self) -> ProgramResult {
        if !self.callback_signer.is_signer() {
            debug_log!("Missing authority to execute callback!");
            return Err(ProgramError::MissingRequiredSignature);
        }
        require_eq_keys!(
            self.callback_signer.address(),
            &CALLBACK_SIGNER,
            ProgramError::IncorrectAuthority
        );
        require_eq_keys!(
            self.magic_context_info.address(),
            &MAGIC_CONTEXT_ID,
            ProgramError::InvalidSeeds
        );
        require_eq_keys!(
            self.magic_program_info.address(),
            &MAGIC_PROGRAM_ID,
            ProgramError::InvalidSeeds
        );
        require_owned_by!(self.queue_info, &crate::ID);

        Ok(())
    }
}

pub fn process_refund_on_failure_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [callback_signer, refund_destination_owner, queue_info, magic_fee_vault_info, magic_context_info, magic_program_info] =
        require_n_accounts!(accounts, 6);
    let accounts = RefundOnFailureAccounts {
        callback_signer,
        refund_destination_owner,
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    };
    accounts.validate()?;

    let response = MagicResponseView::deserialize(instruction_data)?;
    let args = RefundOnFailureArgs::decode(response.data)?;

    if response.ok {
        #[cfg(feature = "logging")]
        {
            use alloc::string::ToString;
            if let Some(signature) = response.signature {
                pinocchio_log::log!(
                    "Amount refunded: {}, signature: {}",
                    args.amount(),
                    signature.to_string().as_str()
                );
            }
        }

        return Ok(());
    }

    #[cfg(feature = "logging")]
    {
        use alloc::string::ToString;
        if let Some(signature) = response.signature {
            pinocchio_log::log!(
                "Failed to refund: {}, signature: {}",
                args.amount(),
                signature.to_string().as_str()
            );
        } else {
            pinocchio_log::log!("Failed to refund: {}", args.amount());
        }
    }

    schedule_refund_on_failure(&accounts, args.amount())
}

pub(crate) fn schedule_refund_on_failure(
    accounts: &RefundOnFailureAccounts<'_>,
    amount: u64,
) -> ProgramResult {
    let RefundOnFailureAccounts {
        refund_destination_owner,
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
        ..
    } = accounts;

    let queue_data = unsafe { queue_info.borrow_unchecked() };
    let (header, _) = queue_views_checked(queue_data)?;

    // Verify
    let derived_magic_fee_vault = magic_fee_vault_pda_from_validator(&header.validator);
    require!(
        derived_magic_fee_vault.to_bytes() == magic_fee_vault_info.address().to_bytes(),
        ProgramError::InvalidSeeds
    );

    // Handle transfer failure by scheduling refund action
    let mint = header.mint;
    let (vault, _) = ephemeral_spl_api::Address::find_program_address(&[mint.as_ref()], &crate::ID);

    // Construct action
    let callback_accounts = create_callback_accounts(
        refund_destination_owner.address(),
        queue_info.address(),
        magic_fee_vault_info.address(),
    );
    let encoded_refund_args = RefundOnFailureArgs { amount }.encode()?;
    let callback = create_callback(&callback_accounts, &encoded_refund_args);

    let action_builder = QueuedTransferActionBuilder::new(
        queue_info,
        refund_destination_owner.address(),
        &vault,
        &mint,
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

#[fixed_offset_layout]
pub(crate) struct RefundOnFailureArgs {
    /// Amount to be refunded
    pub amount: u64,
}

fn create_callback_accounts(
    refund_destination_owner: &ephemeral_spl_api::Address,
    queue: &ephemeral_spl_api::Address,
    magic_fee_vault: &ephemeral_spl_api::Address,
) -> [ShortAccountMeta; 6] {
    [
        ShortAccountMeta {
            pubkey: CALLBACK_SIGNER,
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
