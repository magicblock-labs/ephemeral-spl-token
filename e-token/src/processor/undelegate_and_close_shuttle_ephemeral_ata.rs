use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::instruction::internal::CLOSE_SHUTTLE_ATA_INTENT_V2;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 1280;
const CLOSE_SHUTTLE_ATA_COMPUTE_UNITS: u32 = 70_000;

/// Commit and undelegate shuttle wallet ATA, then schedule a post-undelegate
/// action that closes shuttle wallet ATA and shuttle EATA if amount == 0, then
/// closes shuttle metadata account and sends rent to payer.
///
/// Expected accounts (in order used below):
/// 0. [signer]   Payer (must match shuttle.payer)
/// 1. []         Shuttle ephemeral ata account (PDA [owner, mint, shuttle_id])
/// 2. []         Shuttle EATA account
/// 3. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
/// 4. []         Token program account
/// 5. [writable] Magic context account
/// 6. []         Magic program
pub fn process_undelegate_and_close_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let escrow_index = parse_escrow_index(instruction_data)?;

    let [payer, shuttle_info, shuttle_ephemeral_ata_info, shuttle_wallet_ata_info, token_program_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
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

    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
    if !shuttle.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle.payer != *payer.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    let mint = {
        let shuttle_ephemeral_ata = unsafe {
            load_unchecked::<EphemeralAta>(shuttle_ephemeral_ata_info.borrow_unchecked())?
        };
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

    {
        let shuttle_wallet_data = unsafe { shuttle_wallet_ata_info.borrow_unchecked() };
        if shuttle_wallet_data.len() < TokenAccount::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let shuttle_wallet = unsafe { TokenAccount::from_bytes_unchecked(shuttle_wallet_data) };
        if !shuttle_wallet.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if shuttle_wallet.owner() != shuttle_info.address() || shuttle_wallet.mint() != &mint {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    unsafe {
        if shuttle_wallet_ata_info
            .owner()
            .ne(token_program_info.address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    undelegate_and_close_shuttle_ephemeral_ata(
        payer,
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
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

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn undelegate_and_close_shuttle_ephemeral_ata(
    payer: &AccountView,
    shuttle_info: &AccountView,
    shuttle_ephemeral_ata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    token_program_info: &AccountView,
    magic_context: &AccountView,
    magic_program: &AccountView,
    escrow_index: u8,
) -> ProgramResult {
    let close_handler_data = [CLOSE_SHUTTLE_ATA_INTENT_V2, escrow_index];
    let close_handler_accounts = [
        ShortAccountMeta {
            pubkey: *payer.address(),
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
            pubkey: *token_program_info.address(),
            is_writable: token_program_info.is_writable(),
        },
    ];
    let close_handler = [CallHandler {
        destination_program: Address::new_from_array(crate::ID),
        escrow_authority: payer.clone(),
        args: ActionArgs::new(&close_handler_data).with_escrow_index(escrow_index),
        compute_units: CLOSE_SHUTTLE_ATA_COMPUTE_UNITS,
        accounts: &close_handler_accounts,
    }];
    let committed_accounts = [shuttle_wallet_ata_info.clone()];
    let mut intent_bundle_data = [0u8; INTENT_BUNDLE_DATA_BUF_SIZE];

    MagicIntentBundleBuilder::new(payer.clone(), magic_context.clone(), magic_program.clone())
        .commit_and_undelegate(&committed_accounts)
        .add_post_undelegate_actions(&close_handler)
        .build_and_invoke(&mut intent_bundle_data)
}
