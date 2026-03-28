use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::instruction::internal::CLOSE_SHUTTLE_ATA_INTENT;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::utils::{get_associated_token_address, validate_token_account};

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1280;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 70_000;

/// Commit and undelegate shuttle wallet ATA, then schedule a post-undelegate
/// action that closes shuttle wallet ATA and shuttle EATA if amount == 0, then
/// closes shuttle metadata account and sends rent to the stored reimbursement recipient.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Executor payer
/// 1. [writable] Rent reimbursement account (must match shuttle.payer)
/// 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
/// 3. []         Shuttle EATA account
/// 4. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
/// 5. [writable] Refund token ATA
/// 6. []         Token program account
/// 7. [writable] Magic context account
/// 8. []         Magic program
pub fn process_undelegate_and_close_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;

    let [executor, rent_reimbursement, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, refund_token_info, token_program_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !executor.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    unsafe {
        if shuttle_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
    if shuttle.payer != *rent_reimbursement.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let mint = {
        let shuttle_ephemeral_ata = load_initialized::<EphemeralAta>(unsafe {
            shuttle_ephemeral_ata_info.borrow_unchecked()
        })?;
        if shuttle_ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_ephemeral_ata.mint.clone();
        mint
    };

    let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_shuttle_ephemeral_ata != *shuttle_ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let expected_shuttle_wallet_ata =
        get_associated_token_address(shuttle_info.address(), &mint, token_program_info.address());
    if expected_shuttle_wallet_ata != *shuttle_wallet_ata_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    validate_token_account(
        shuttle_wallet_ata_info,
        &mint,
        Some(shuttle_info.address()),
        Some(token_program_info.address()),
    )?;
    validate_token_account(
        refund_token_info,
        &mint,
        Some(&shuttle.owner),
        Some(token_program_info.address()),
    )?;

    undelegate_and_close_shuttle_ephemeral_ata(
        executor,
        rent_reimbursement,
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
        refund_token_info,
        &mint,
        token_program_info,
        magic_context,
        magic_program,
        escrow_index,
    )
}

#[inline(always)]
fn parse_escrow_index(instruction_data: &[u8]) -> Result<u8, ProgramError> {
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
fn undelegate_and_close_shuttle_ephemeral_ata(
    executor: &AccountView,
    rent_reimbursement: &AccountView,
    shuttle_info: &AccountView,
    shuttle_ephemeral_ata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    refund_token_info: &AccountView,
    mint: &Address,
    token_program_info: &AccountView,
    magic_context: &AccountView,
    magic_program: &AccountView,
    escrow_index: u8,
) -> ProgramResult {
    let (vault_info, _) = ephemeral_spl_api::Address::find_program_address(
        &[mint.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    let vault_token_info =
        get_associated_token_address(&vault_info, mint, token_program_info.address());
    // The close intent now always expects the expanded withdraw-capable shape.
    let close_handler_data = [CLOSE_SHUTTLE_ATA_INTENT, escrow_index];
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
            pubkey: *refund_token_info.address(),
            is_writable: refund_token_info.is_writable(),
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
            is_writable: token_program_info.is_writable(),
        },
    ];
    let close_handler = [CallHandler {
        destination_program: Address::new_from_array(crate::ID),
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
