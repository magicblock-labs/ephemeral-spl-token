use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::instruction::internal::CLOSE_SHUTTLE_ATA_INTENT;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleMetadata,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1536;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 100_000;

/// Commit and undelegate a shuttle wallet ATA, then schedule a post-undelegate
/// close/refund action for the shuttle accounts.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Executor payer
/// 1. [writable] Rent reimbursement account (must match shuttle.payer)
/// 2. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
/// 3. []         Shuttle EATA account
/// 4. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
/// 5. [writable] Destination owner ATA
/// 6. []         Mint account
/// 7. []         Token program account
/// 8. [writable] Magic context account
/// 9. []         Magic program
pub fn process_undelegate_withdraw_and_close_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;

    let [executor, rent_reimbursement, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, destination_token_info, mint_info, token_program_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !executor.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    unsafe {
        if shuttle_info.owner().ne(&ephemeral_spl_api::ID) {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let shuttle = unsafe { load_unchecked::<ShuttleMetadata>(shuttle_info.borrow_unchecked())? };
    if !shuttle.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle.payer != *rent_reimbursement.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let shuttle_ephemeral_ata =
        unsafe { load_unchecked::<EphemeralAta>(shuttle_ephemeral_ata_info.borrow_unchecked())? };
    if !shuttle_ephemeral_ata.is_initialized()
        || shuttle_ephemeral_ata.owner != *shuttle_info.address()
        || shuttle_ephemeral_ata.mint != *mint_info.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let expected_shuttle_wallet_ata = get_associated_token_address(
        shuttle_info.address(),
        mint_info.address(),
        token_program_info.address(),
    );
    if expected_shuttle_wallet_ata != *shuttle_wallet_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    validate_token_account(
        shuttle_wallet_ata_info,
        token_program_info,
        mint_info,
        shuttle_info,
    )?;
    validate_destination_token_account(destination_token_info, token_program_info, mint_info)?;

    undelegate_withdraw_and_close_shuttle_ephemeral_ata(
        executor,
        rent_reimbursement,
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
        destination_token_info,
        mint_info,
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

#[inline(always)]
fn get_associated_token_address(
    wallet: &Address,
    mint: &Address,
    token_program: &Address,
) -> Address {
    ephemeral_spl_api::Address::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        &pinocchio_associated_token_account::id(),
    )
    .0
}

#[inline(always)]
fn validate_token_account(
    account_info: &AccountView,
    token_program_info: &AccountView,
    mint_info: &AccountView,
    owner_info: &AccountView,
) -> ProgramResult {
    if !account_info.owned_by(token_program_info.address()) {
        return Err(ProgramError::IllegalOwner);
    }

    let account_data = unsafe { account_info.borrow_unchecked() };
    if account_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let token = unsafe { TokenAccount::from_bytes_unchecked(account_data) };
    if !token.is_initialized()
        || token.owner() != owner_info.address()
        || token.mint() != mint_info.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

#[inline(always)]
fn validate_destination_token_account(
    destination_token_info: &AccountView,
    token_program_info: &AccountView,
    mint_info: &AccountView,
) -> ProgramResult {
    if !destination_token_info.owned_by(token_program_info.address()) {
        return Err(ProgramError::IllegalOwner);
    }

    let destination_data = unsafe { destination_token_info.borrow_unchecked() };
    if destination_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let destination = unsafe { TokenAccount::from_bytes_unchecked(destination_data) };
    if !destination.is_initialized() || destination.mint() != mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn undelegate_withdraw_and_close_shuttle_ephemeral_ata(
    executor: &AccountView,
    rent_reimbursement: &AccountView,
    shuttle_info: &AccountView,
    shuttle_ephemeral_ata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    destination_token_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    magic_context: &AccountView,
    magic_program: &AccountView,
    escrow_index: u8,
) -> ProgramResult {
    let (vault_info, _) = GlobalVault::find_pda(&mint_info.address());
    let vault_token_info = get_associated_token_address(
        &vault_info,
        mint_info.address(),
        token_program_info.address(),
    );
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
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *destination_token_info.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *mint_info.address(),
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
