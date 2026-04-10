use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::instruction::internal::SETTLE_AND_CLOSE_SHUTTLE_INTENT;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use pinocchio::address::address_eq;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::assert_owner;
use crate::processor::utils::{get_associated_token_address, validate_token_account};

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1536;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 100_000;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Executor payer.
///  1: [writable]          - Any     : Rent reimbursement account (must match `shuttle.payer`).
///  2: []                  - PDA     : Shuttle metadata account (PDA derived from [owner, mint, shuttle_id]).
///  3: []                  - PDA     : Shuttle EATA account.
///  4: [writable]          - SPL     : Shuttle wallet ATA account (ATA for [shuttle_metadata, mint]).
///  5: [writable]          - SPL     : Refund token ATA.
///  6: []                  - SPL     : Token program account.
///  7: [writable]          - Any     : Magic context account.
///  8: []                  - Program : Magic program.
///
/// Instruction Data: optional escrow_index (u8)
///
pub fn process_undelegate_and_close_shuttle_to_owner(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;

    let [executor, rent_reimbursement, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, refund_token_info, token_program_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // TODO (snawaz):  unauthorized third-party cleanup/cancellation is possible.
    //
    // it is currently permissionless, means anyone (executor) could
    // force undelegate-and-close shuttle of other users and force shuttle into refund/cleanup.
    //
    // enforce: executor == shuttle.owner
    if !executor.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    assert_owner!(shuttle_info, &crate::ID);

    let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
    if shuttle.payer != *rent_reimbursement.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let mint = {
        let shuttle_ephemeral_ata = load_initialized::<EphemeralAta>(unsafe {
            shuttle_ephemeral_ata_info.borrow_unchecked()
        })?;
        if !address_eq(&shuttle_ephemeral_ata.owner, shuttle_info.address()) {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_ephemeral_ata.mint.clone();
        mint
    };

    let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref()],
        &crate::ID,
    );
    if !address_eq(
        &derived_shuttle_ephemeral_ata,
        shuttle_ephemeral_ata_info.address(),
    ) {
        return Err(ProgramError::InvalidSeeds);
    }

    let expected_shuttle_wallet_ata =
        get_associated_token_address(shuttle_info.address(), &mint, token_program_info.address());
    if !address_eq(
        &expected_shuttle_wallet_ata,
        shuttle_wallet_ata_info.address(),
    ) {
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

    schedule_shuttle_close_after_undelegate(
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
fn schedule_shuttle_close_after_undelegate(
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
