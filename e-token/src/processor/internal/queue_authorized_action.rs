use alloc::vec::Vec;

use ephemeral_rollups_pinocchio::{
    intent_bundle::{ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta},
    pda::magic_fee_vault_pda_from_validator,
};
use ephemeral_spl_api::{instructions::ExecuteQueuedTransferArgs, require, state::transfer_queue::QUEUE_SEED};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;
use solana_address::{address_eq, Address};
use spl_token_interface::ID as SPL_TOKEN_PROGRAM_ID;
use wheels::layout::Encodable as _;

use crate::processor::{
    internal,
    internal::{
        rent_pda::RENT_PDA,
        unwrap_pda::{NATIVE_MINT, UNWRAP_PDA},
    },
};

const MAGIC_INTENT_BUNDLE_DATA_LEN: usize = 512;
pub(crate) const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;

pub(crate) struct IntentBundleAccounts<'a> {
    pub queue_info: &'a AccountView,
    pub magic_fee_vault_info: &'a AccountView,
    pub magic_context_info: &'a AccountView,
    pub magic_program_info: &'a AccountView,
}

pub(crate) struct QueueSignerState {
    pub mint: Address,
    pub queue_bump: u8,
    pub validator: Address,
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
    let derived_magic_fee_vault = magic_fee_vault_pda_from_validator(&state.validator);
    require!(
        derived_magic_fee_vault.to_bytes() == magic_accounts.magic_fee_vault_info.address().to_bytes(),
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
    accounts: Vec<ShortAccountMeta>,
    data: Vec<u8>,
}

impl QueuedTransferActionBuilder {
    const EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS: u32 = 140_000;

    pub(crate) fn new(
        queue_info: &AccountView,
        destination_owner: &Address,
        vault: &Address,
        mint: &Address,
        token_program: &Address,
        args: ExecuteQueuedTransferArgs,
    ) -> Self {
        let data =
            crate::instruction::ESplInternalInstruction::ExecuteReadyQueuedTransfer.with_data(&args.encode().unwrap());

        Self {
            queue_info: queue_info.clone(),
            accounts: Self::action_accounts(destination_owner, vault, mint, token_program),
            data,
        }
    }

    pub(crate) fn build(&self) -> CallHandler<'_> {
        CallHandler {
            destination_program: crate::ID,
            escrow_authority: self.queue_info.clone(),
            args: ActionArgs::new(&self.data).with_escrow_index(EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX),
            compute_units: Self::EXECUTE_READY_QUEUED_TRANSFER_COMPUTE_UNITS,
            accounts: &self.accounts,
            callback: None,
        }
    }

    fn action_accounts(
        destination_owner: &Address,
        vault: &Address,
        mint: &Address,
        token_program: &Address,
    ) -> Vec<ShortAccountMeta> {
        let deliver_native = address_eq(mint, &NATIVE_MINT) && address_eq(token_program, &SPL_TOKEN_PROGRAM_ID);
        let vault_token_account = internal::get_associated_token_address(vault, mint, token_program);
        let destination_token_account = internal::get_associated_token_address(destination_owner, mint, token_program);

        // DLP appends [source_program, escrow_authority, escrow_signer].
        let mut accounts = alloc::vec![
            ShortAccountMeta {
                pubkey: *vault,
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: *mint,
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: vault_token_account,
                is_writable: true,
            },
            ShortAccountMeta {
                pubkey: *destination_owner,
                is_writable: deliver_native,
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
                pubkey: *token_program,
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: pinocchio_associated_token_account::ID,
                is_writable: false,
            },
            ShortAccountMeta {
                pubkey: SYSTEM_PROGRAM_ID,
                is_writable: false,
            },
        ];

        if deliver_native {
            accounts.push(ShortAccountMeta {
                pubkey: internal::get_associated_token_address(&UNWRAP_PDA, mint, token_program),
                is_writable: true,
            });
            accounts.push(ShortAccountMeta {
                pubkey: UNWRAP_PDA,
                is_writable: false,
            });
        }

        accounts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_delivery_only_extends_the_wsol_action_accounts() {
        let destination = Address::new_from_array([1; 32]);
        let vault = Address::new_from_array([2; 32]);

        let native_accounts =
            QueuedTransferActionBuilder::action_accounts(&destination, &vault, &NATIVE_MINT, &SPL_TOKEN_PROGRAM_ID);
        assert_eq!(native_accounts.len(), 11);
        assert!(native_accounts[3].is_writable);
        assert_eq!(
            native_accounts[9].pubkey,
            internal::get_associated_token_address(&UNWRAP_PDA, &NATIVE_MINT, &SPL_TOKEN_PROGRAM_ID)
        );
        assert_eq!(native_accounts[10].pubkey, UNWRAP_PDA);

        let spl_accounts = QueuedTransferActionBuilder::action_accounts(
            &destination,
            &vault,
            &Address::new_from_array([3; 32]),
            &SPL_TOKEN_PROGRAM_ID,
        );
        assert_eq!(spl_accounts.len(), 9);
        assert!(!spl_accounts[3].is_writable);
    }
}
