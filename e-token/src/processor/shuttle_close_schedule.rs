use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::instruction::internal::SETTLE_AND_CLOSE_SHUTTLE_INTENT;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::utils::get_associated_token_address;

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1536;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 100_000;

#[inline(always)]
pub(crate) fn parse_escrow_index(instruction_data: &[u8]) -> Result<u8, ProgramError> {
    if instruction_data.is_empty() {
        return Ok(DEFAULT_ESCROW_INDEX);
    }
    if instruction_data.len() != 1 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(instruction_data[0])
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn schedule_shuttle_close_after_undelegate(
    executor: &AccountView,
    rent_reimbursement: &AccountView,
    shuttle_info: &AccountView,
    shuttle_ephemeral_ata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    destination_token_info: &AccountView,
    mint: &Address,
    token_program_info: &AccountView,
    magic_context: &AccountView,
    magic_program: &AccountView,
    escrow_index: u8,
) -> ProgramResult {
    let (vault_info, _) =
        ephemeral_spl_api::Address::find_program_address(&[mint.as_ref()], &crate::ID);
    let vault_token_info =
        get_associated_token_address(&vault_info, mint, token_program_info.address());
    let close_handler_data = [SETTLE_AND_CLOSE_SHUTTLE_INTENT, escrow_index];
    let close_handler_accounts = [
        ShortAccountMeta {
            pubkey: *rent_reimbursement.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *shuttle_info.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *shuttle_ephemeral_ata_info.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *shuttle_wallet_ata_info.address(),
            is_writable: shuttle_wallet_ata_info.is_writable(),
        },
        ShortAccountMeta {
            pubkey: *destination_token_info.address(),
            is_writable: destination_token_info.is_writable(),
        },
        ShortAccountMeta {
            pubkey: *mint,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: vault_info,
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: vault_token_info,
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *token_program_info.address(),
            is_writable: false,
        },
    ];
    let close_handler = [CallHandler {
        destination_program: crate::ID,
        escrow_authority: executor.clone(),
        args: ActionArgs::new(&close_handler_data).with_escrow_index(escrow_index),
        compute_units: CLOSE_SHUTTLE_ATA_COMPUTE_UNITS,
        accounts: &close_handler_accounts,
    }];
    let committed_accounts = [shuttle_wallet_ata_info.clone()];
    let mut intent_bundle_data = [0u8; INTENT_BUNDLE_DATA_BUF_SIZE];

    MagicIntentBundleBuilder::new(
        executor.clone(),
        magic_context.clone(),
        magic_program.clone(),
    )
    .commit_and_undelegate(&committed_accounts)
    .add_post_undelegate_actions(&close_handler)
    .build_and_invoke(&mut intent_bundle_data)
}
