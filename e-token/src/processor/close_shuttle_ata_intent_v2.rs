use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::instructions::CloseAccount;
use pinocchio_token_2022::state::TokenAccount;

pub const CLOSE_SHUTTLE_ATA_INTENT_V2: u8 = 197;
const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";

/// Post-undelegate handler that closes shuttle wallet ATA and shuttle EATA
/// when amounts are 0, then closes shuttle metadata account.
///
/// Expected leading accounts:
/// 0. [writable] Shuttle payer account (must equal `ShuttleEphemeralAta.payer`)
/// 1. [writable] Shuttle metadata account
/// 2. [writable] Shuttle EATA account (PDA [shuttle_metadata, mint])
/// 3. [writable] Shuttle wallet ATA account
/// 4. []         Token program account
///
/// Expected trailing accounts from tail:
/// - second to last: escrow authority
/// - last:          escrow signer PDA
pub fn process_close_shuttle_ata_intent_v2(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [escrow_index] = instruction_data else {
        return Err(ProgramError::InvalidInstructionData);
    };

    if accounts.len() < 7 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let shuttle_payer_info = &accounts[0];
    let shuttle_info = &accounts[1];
    let shuttle_ephemeral_ata_info = &accounts[2];
    let shuttle_wallet_ata_info = &accounts[3];
    let token_program_info = &accounts[4];
    let escrow_authority = &accounts[accounts.len() - 2];
    let escrow_signer = &accounts[accounts.len() - 1];

    if escrow_authority.address() != shuttle_payer_info.address() {
        return Err(ProgramError::IncorrectAuthority);
    }
    if !escrow_signer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let escrow_index_seed = [*escrow_index];
    let (expected_escrow, _) = ephemeral_spl_api::Address::find_program_address(
        &[
            DLP_EPHEMERAL_BALANCE_TAG,
            escrow_authority.address().as_ref(),
            escrow_index_seed.as_ref(),
        ],
        &ephemeral_rollups_pinocchio::ID,
    );
    if expected_escrow != *escrow_signer.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let shuttle_present = shuttle_info.lamports() > 0;
    let shuttle_ephemeral_present = shuttle_ephemeral_ata_info.lamports() > 0;
    let shuttle_wallet_present = shuttle_wallet_ata_info.lamports() > 0;
    if !shuttle_present && !shuttle_ephemeral_present && !shuttle_wallet_present {
        return Ok(());
    }

    let mut shuttle_id = 0u32;
    let mut shuttle_owner_opt = None;
    if shuttle_present {
        if !shuttle_info.owned_by(&ephemeral_spl_api::program::id_address()) {
            return Err(ProgramError::IllegalOwner);
        }
        let shuttle =
            unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
        if !shuttle.is_initialized() {
            return Err(ProgramError::InvalidAccountData);
        }
        if shuttle.payer != *shuttle_payer_info.address() {
            return Err(ProgramError::IncorrectAuthority);
        }
        shuttle_id = shuttle.id;
        let shuttle_owner = shuttle.owner.clone();
        shuttle_owner_opt = Some(shuttle_owner);
    }

    if shuttle_wallet_present {
        if !shuttle_wallet_ata_info.owned_by(token_program_info.address()) {
            return Err(ProgramError::IllegalOwner);
        }
        if !shuttle_present {
            return Err(ProgramError::InvalidAccountData);
        }

        let shuttle_owner = shuttle_owner_opt
            .as_ref()
            .ok_or(ProgramError::InvalidAccountData)?;
        let (mint, shuttle_wallet_amount) = {
            let shuttle_wallet_data = unsafe { shuttle_wallet_ata_info.borrow_unchecked() };
            if shuttle_wallet_data.len() < TokenAccount::BASE_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let shuttle_wallet = unsafe { TokenAccount::from_bytes_unchecked(shuttle_wallet_data) };
            if !shuttle_wallet.is_initialized() {
                return Err(ProgramError::UninitializedAccount);
            }
            if shuttle_wallet.owner() != shuttle_info.address() {
                return Err(ProgramError::InvalidAccountData);
            }
            let mint = shuttle_wallet.mint();
            (mint, shuttle_wallet.amount())
        };

        if shuttle_wallet_amount != 0 {
            return Err(ProgramError::InvalidArgument);
        }

        let shuttle_id_seed = shuttle_id.to_le_bytes();
        let (derived_shuttle, shuttle_bump) = ephemeral_spl_api::Address::find_program_address(
            &[
                shuttle_owner.as_ref(),
                mint.as_ref(),
                shuttle_id_seed.as_ref(),
            ],
            &ephemeral_spl_api::program::id_address(),
        );
        if derived_shuttle != *shuttle_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        let bump = [shuttle_bump];
        let signer_seeds = [
            Seed::from(shuttle_owner.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(shuttle_id_seed.as_ref()),
            Seed::from(&bump),
        ];
        let signer = Signer::from(&signer_seeds);

        CloseAccount {
            account: shuttle_wallet_ata_info,
            destination: shuttle_payer_info,
            authority: shuttle_info,
            token_program: token_program_info.address(),
        }
        .invoke_signed(&[signer])?;
    }

    if shuttle_ephemeral_present {
        if !shuttle_ephemeral_ata_info.owned_by(&ephemeral_spl_api::program::id_address()) {
            return Err(ProgramError::IllegalOwner);
        }
        if !shuttle_present {
            return Err(ProgramError::InvalidAccountData);
        }

        let shuttle_owner = shuttle_owner_opt
            .as_ref()
            .ok_or(ProgramError::InvalidAccountData)?;
        let (mint, shuttle_ephemeral_amount) = {
            let shuttle_ephemeral_ata = unsafe {
                load_unchecked::<EphemeralAta>(shuttle_ephemeral_ata_info.borrow_unchecked())?
            };
            if !shuttle_ephemeral_ata.is_initialized() {
                return Err(ProgramError::InvalidAccountData);
            }
            if shuttle_ephemeral_ata.owner != *shuttle_info.address() {
                return Err(ProgramError::InvalidAccountData);
            }
            #[allow(clippy::clone_on_copy)]
            let mint = shuttle_ephemeral_ata.mint.clone();
            (mint, shuttle_ephemeral_ata.amount)
        };

        if shuttle_ephemeral_amount != 0 {
            return Err(ProgramError::InvalidArgument);
        }

        let shuttle_id_seed = shuttle_id.to_le_bytes();
        let (derived_shuttle, _) = ephemeral_spl_api::Address::find_program_address(
            &[
                shuttle_owner.as_ref(),
                mint.as_ref(),
                shuttle_id_seed.as_ref(),
            ],
            &ephemeral_spl_api::program::id_address(),
        );
        if derived_shuttle != *shuttle_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        let (derived_shuttle_ephemeral_ata, _) = ephemeral_spl_api::Address::find_program_address(
            &[shuttle_info.address().as_ref(), mint.as_ref()],
            &ephemeral_spl_api::program::id_address(),
        );
        if derived_shuttle_ephemeral_ata != *shuttle_ephemeral_ata_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        close_program_account_to_recipient(shuttle_ephemeral_ata_info, shuttle_payer_info)?;
    }

    if shuttle_present {
        close_program_account_to_recipient(shuttle_info, shuttle_payer_info)?;
    }

    Ok(())
}

#[inline(always)]
fn close_program_account_to_recipient(
    account: &AccountView,
    recipient: &AccountView,
) -> ProgramResult {
    if *recipient.address() == *account.address() {
        return Err(ProgramError::InvalidArgument);
    }

    let lamports_to_refund = account.lamports();
    let updated_recipient_lamports = recipient
        .lamports()
        .checked_add(lamports_to_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    recipient.set_lamports(updated_recipient_lamports);
    account.set_lamports(0);
    account.close()?;
    Ok(())
}
