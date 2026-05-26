use crate::processor::utils::{get_associated_token_address, validate_token_account};
use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::instruction::ESplInternalInstruction;

pub(crate) const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1536;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 100_000;
const CLOSE_STASH_DATA_LEN: usize = 33;

struct CloseStashForward {
    user: [u8; 32],
    stash_bump: u8,
}

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
/// Instruction Data: optional escrow_index (u8), optionally followed by
/// `[user(32) | stash_bump(1)]` for the stash close path. In that path the
/// executor is the stash PDA and account 1 is the rent sink.
///
pub fn process_undelegate_and_close_shuttle_to_owner(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let head_accounts = match accounts.len() {
        9 | 11 => &accounts[..9],
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };
    let [
        executor, // force multi-line
        rent_reimbursement,
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
        refund_token_info,
        token_program_info,
        magic_context,
        magic_program,
    ] = require_n_accounts!(head_accounts, 9);

    let (escrow_index, close_stash_seeds) = parse_instruction_data(instruction_data)?;
    if close_stash_seeds.is_some() && accounts.len() == 11 {
        require_eq_keys!(
            accounts[9].address(),
            executor.address(),
            ProgramError::InvalidSeeds
        );
        require_eq_keys!(
            accounts[10].address(),
            rent_reimbursement.address(),
            ProgramError::InvalidSeeds
        );
    }
    let close_stash =
        close_stash_seeds.map(|(user, stash_bump)| CloseStashForward { user, stash_bump });

    // TODO (snawaz):  unauthorized third-party cleanup/cancellation is possible.
    //
    // it is currently permissionless, means anyone (executor) could
    // force undelegate-and-close shuttle of other users and force shuttle into refund/cleanup.
    //
    // enforce: executor == shuttle.owner
    require!(executor.is_signer(), ProgramError::MissingRequiredSignature);

    require!(
        shuttle_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
    require_eq_keys!(
        &shuttle.payer,
        rent_reimbursement.address(),
        ProgramError::IncorrectAuthority
    );

    let mint = {
        let shuttle_ephemeral_ata = load_initialized::<EphemeralAta>(unsafe {
            shuttle_ephemeral_ata_info.borrow_unchecked()
        })?;
        require_eq_keys!(
            &shuttle_ephemeral_ata.owner,
            shuttle_info.address(),
            ProgramError::InvalidAccountData
        );
        #[allow(clippy::clone_on_copy)]
        let mint = shuttle_ephemeral_ata.mint.clone();
        mint
    };

    let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref()],
        &crate::ID,
    );
    require_eq_keys!(
        &derived_shuttle_ephemeral_ata,
        shuttle_ephemeral_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    let expected_shuttle_wallet_ata =
        get_associated_token_address(shuttle_info.address(), &mint, token_program_info.address());
    require_eq_keys!(
        &expected_shuttle_wallet_ata,
        shuttle_wallet_ata_info.address(),
        ProgramError::InvalidAccountData
    );

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
        close_stash.as_ref(),
    )
}

#[inline(always)]
fn parse_instruction_data(
    instruction_data: &[u8],
) -> Result<(u8, Option<([u8; 32], u8)>), ProgramError> {
    let (escrow_index, tail) = match instruction_data.split_first() {
        None => (DEFAULT_ESCROW_INDEX, &[][..]),
        Some((first, rest)) => (*first, rest),
    };
    let close_stash = match tail.len() {
        0 => None,
        n if n == CLOSE_STASH_DATA_LEN => {
            let mut user = [0u8; 32];
            user.copy_from_slice(&tail[0..32]);
            Some((user, tail[32]))
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    Ok((escrow_index, close_stash))
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
    close_stash: Option<&CloseStashForward>,
) -> ProgramResult {
    let (vault_info, _) =
        ephemeral_spl_api::Address::find_program_address(&[mint.as_ref()], &crate::ID);
    let vault_token_info =
        get_associated_token_address(&vault_info, mint, token_program_info.address());
    let mut close_handler_data =
        ESplInternalInstruction::SettleAndCloseShuttleIntent.with_data(&[escrow_index]);
    if let Some(close) = close_stash {
        close_handler_data.extend_from_slice(&close.user);
        close_handler_data.push(close.stash_bump);
    }
    let close_handler_accounts = alloc::vec![
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
            is_writable: true,
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
        callback: None,
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
