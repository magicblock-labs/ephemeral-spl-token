use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::internal;
use crate::processor::internal::ASSOCIATED_TOKEN_PROGRAM_ID;
use crate::ExecuteQueuedTransferArgs;
use alloc::vec;
use alloc::vec::Vec;
use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, ActionCallback, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_rollups_pinocchio::spl::TOKEN_PROGRAM_ID;
use ephemeral_spl_api::require;
use ephemeral_spl_api::state::transfer_queue::{QueuedTransfer, QUEUE_SEED};
use hydra_api::instruction::SYSTEM_PROGRAM_ID;
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

const MAGIC_INTENT_BUNDLE_DATA_LEN: usize = 512;
pub(crate) const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;
pub(crate) const EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS: u32 = 140_000;

pub(crate) struct MagicAccounts<'a> {
    pub queue_info: &'a AccountView,
    pub magic_fee_vault_info: &'a AccountView,
    pub magic_context_info: &'a AccountView,
    pub magic_program_info: &'a AccountView,
}

pub(crate) struct MagicState {
    pub mint: ephemeral_spl_api::Address,
    pub queue_bump: u8,
    pub validator: ephemeral_spl_api::Address,
}

#[inline(always)]
pub(crate) fn invoke_standalone_transfer_action(
    magic_accounts: &MagicAccounts<'_>,
    state: &MagicState,
    standalone_actions: &[CallHandler],
) -> ProgramResult {
    let queue_bump_seed = [state.queue_bump];
    let signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(state.mint.as_ref()),
        Seed::from(state.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let signers = [Signer::from(&signer_seeds)];
    let mut intent_bundle_data = [0_u8; MAGIC_INTENT_BUNDLE_DATA_LEN];
    let derived_magic_fee_vault =
        magic_fee_vault_pda_from_validator(&state.validator.to_bytes().into());
    require!(
        derived_magic_fee_vault.to_bytes()
            == magic_accounts.magic_fee_vault_info.address().to_bytes(),
        ProgramError::InvalidSeeds
    );

    MagicIntentBundleBuilder::new(
        magic_accounts.queue_info.clone(),
        magic_accounts.magic_context_info.clone(),
        magic_accounts.magic_program_info.clone(),
    )
    .magic_fee_vault(magic_accounts.magic_fee_vault_info.clone())
    .set_standalone_actions(standalone_actions)
    .build_and_invoke_signed(&mut intent_bundle_data, &signers)
}

pub(crate) fn execute_queued_transfer_action<'a>(
    queue_info: &AccountView,
    action_accounts: &'a [ShortAccountMeta],
    action_data: &'a [u8],
) -> CallHandler<'a> {
    CallHandler {
        destination_program: crate::ID,
        escrow_authority: queue_info.clone(),
        args: ActionArgs::new(action_data)
            .with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
        compute_units: EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
        accounts: action_accounts,
        callback: None,
    }
}

pub(crate) fn create_action_accounts(
    destination_owner: &ephemeral_spl_api::Address,
    vault: &ephemeral_spl_api::Address,
    mint: &ephemeral_spl_api::Address,
) -> [ShortAccountMeta; 9] {
    let vault_token_account = internal::derive_associated_token_address(vault, mint);
    let destination_token_account =
        internal::derive_associated_token_address(&destination_owner, mint);

    // Note that we initialize CallHandler with 9 accounts only, and then 3 more accounts [source_program,
    // escrow_authority, escrow_signer] are appended by DLP's CallHandlerV2 instruction, which is
    // why EXECUTE_READY_QUEUED_TRANSFER receives 12 accounts (not 9).
    [
        ShortAccountMeta {
            pubkey: vault.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: mint.clone(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: vault_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *destination_owner,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: destination_token_account,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: RENT_PDA,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: TOKEN_PROGRAM_ID,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: ASSOCIATED_TOKEN_PROGRAM_ID,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: SYSTEM_PROGRAM_ID,
            is_writable: false,
        },
    ]
}

// pub(crate) struct ActionBuilder {
//     args: ExecuteQueuedTransferArgs,
//     data: Vec<u8>,
//     accounts: Vec<ShortAccountMeta>,
// }
//
// impl ActionBuilder {
//     pub(crate) fn new(args: ExecuteQueuedTransferArgs) -> Self {
//         Self {
//             args,
//             data: Vec::new(),
//             accounts: Vec::new()
//         }
//     }
//
//     pub(crate) fn action(
//         &self,
//         destination_owner: &ephemeral_spl_api::Address,
//         vault: &ephemeral_spl_api::Address,
//         mint: &ephemeral_spl_api::Address,) -> CallHandler<'_> {
//
//     }
// }
