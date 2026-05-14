use alloc::vec::Vec;

use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::internal;
use crate::processor::internal::ASSOCIATED_TOKEN_PROGRAM_ID;
use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_rollups_pinocchio::spl::TOKEN_PROGRAM_ID;
use ephemeral_spl_api::require;
use ephemeral_spl_api::state::transfer_queue::QUEUE_SEED;
use hydra_api::instruction::SYSTEM_PROGRAM_ID;
use pinocchio::cpi::{Seed, Signer};
use pinocchio::error::ProgramError;
use pinocchio::{AccountView, ProgramResult};

const MAGIC_INTENT_BUNDLE_DATA_LEN: usize = 512;

pub(crate) struct IntentBundleAccounts<'a> {
    pub queue_info: &'a AccountView,
    pub magic_fee_vault_info: &'a AccountView,
    pub magic_context_info: &'a AccountView,
    pub magic_program_info: &'a AccountView,
}

pub(crate) struct QueueSignerState {
    pub mint: ephemeral_spl_api::Address,
    pub queue_bump: u8,
    pub validator: ephemeral_spl_api::Address,
}

#[inline(always)]
pub(crate) fn invoke_standalone_action(
    magic_accounts: &IntentBundleAccounts<'_>,
    state: &QueueSignerState,
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

/// Action builder for `ExecuteReadyQueuedTransfer`
pub(crate) struct QueuedTransferActionBuilder {
    queue_info: AccountView,
    accounts: [ShortAccountMeta; 9],
    data: Vec<u8>,
}

impl QueuedTransferActionBuilder {
    const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;
    const EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS: u32 = 140_000;

    pub(crate) fn new(
        queue_info: &AccountView,
        destination_owner: &ephemeral_spl_api::Address,
        vault: &ephemeral_spl_api::Address,
        mint: &ephemeral_spl_api::Address,
        args: crate::ExecuteQueuedTransferArgs,
    ) -> Self {
        let data = crate::instruction::ESplInternalInstruction::ExecuteReadyQueuedTransfer
            .with_data(&args.encode().unwrap());

        Self {
            queue_info: queue_info.clone(),
            accounts: Self::action_accounts(destination_owner, vault, mint),
            data,
        }
    }

    pub(crate) fn build(&self) -> CallHandler<'_> {
        CallHandler {
            destination_program: crate::ID,
            escrow_authority: self.queue_info.clone(),
            args: ActionArgs::new(&self.data)
                .with_escrow_index(Self::EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
            compute_units: Self::EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
            accounts: &self.accounts,
            callback: None,
        }
    }

    fn action_accounts(
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
}
