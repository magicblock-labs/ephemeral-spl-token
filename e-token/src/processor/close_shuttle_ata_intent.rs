use crate::processor::utils::validate_token_account;
use crate::processor::withdraw_spl_tokens::withdraw_ephemeral_ata_tokens;
use crate::{assert_owner, assert_signer};
use ephemeral_spl_api::state::{
    ephemeral_ata::read_ephemeral_ata_compat, load_initialized,
    shuttle_ephemeral_ata::ShuttleMetadata,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::instructions::CloseAccount;
const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";

/// Post-undelegate handler that first withdraws any remaining shuttle EATA
/// balance through the shared vault flow, then closes shuttle wallet ATA,
/// shuttle EATA, and shuttle metadata, refunding rent to the stored payer.
///
/// Expected leading accounts:
/// 0. [writable] Shuttle rent reimbursement account (must equal `ShuttleMetadata.payer`)
/// 1. [writable] Shuttle metadata account
/// 2. [writable] Shuttle EATA account (PDA [shuttle_metadata, mint])
/// 3. [writable] Shuttle wallet ATA account
/// 4. [writable] Destination token account
/// 5. []         Mint account
/// 6. []         Global Vault account
/// 7. [writable] Vault source token account
/// 8. []         Token program account
///
/// Expected trailing accounts from tail:
/// - second to last: escrow authority
/// - last:          escrow signer PDA
pub fn process_close_shuttle_ata_intent(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // TODO(GabrielePicco): ensure we schedule this from the same program id

    let [escrow_index] = instruction_data else {
        return Err(ProgramError::InvalidInstructionData);
    };

    if accounts.len() < 11 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let rent_reimbursement_info = &accounts[0];
    let shuttle_info = &accounts[1];
    let shuttle_ephemeral_ata_info = &accounts[2];
    let shuttle_wallet_ata_info = &accounts[3];
    let destination_token_info = &accounts[4];
    let mint_info = &accounts[5];
    let vault_info = &accounts[6];
    let vault_source_token_acc = &accounts[7];
    let token_program_info = &accounts[8];
    let escrow_authority = &accounts[accounts.len() - 2];
    let escrow_signer = &accounts[accounts.len() - 1];

    assert_signer!(escrow_signer);

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

    let mut shuttle_id = 0u32;
    let mut shuttle_owner_opt = None;
    if shuttle_present {
        assert_owner!(shuttle_info, &ephemeral_spl_api::program::id_address());

        let shuttle =
            load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        if shuttle.payer != *rent_reimbursement_info.address() {
            return Err(ProgramError::IncorrectAuthority);
        }
        shuttle_id = shuttle.id;
        let shuttle_owner = shuttle.owner;
        shuttle_owner_opt = Some(shuttle_owner);
    }

    if shuttle_wallet_present {
        assert_owner!(shuttle_wallet_ata_info, token_program_info.address());
        if !shuttle_present {
            return Err(ProgramError::InvalidAccountData);
        }

        let shuttle_owner = shuttle_owner_opt
            .as_ref()
            .ok_or(ProgramError::InvalidAccountData)?;
        let (mint, shuttle_wallet_amount) = {
            let token_account = validate_token_account(
                shuttle_wallet_ata_info,
                mint_info.address(),
                Some(shuttle_info.address()),
                Some(token_program_info.address()),
            )?;
            (token_account.mint(), token_account.amount())
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
            destination: rent_reimbursement_info,
            authority: shuttle_info,
            token_program: token_program_info.address(),
        }
        .invoke_signed(&[signer])?;
    }

    if shuttle_ephemeral_present {
        assert_owner!(
            shuttle_ephemeral_ata_info,
            &ephemeral_spl_api::program::id_address()
        );
        if !shuttle_present {
            return Err(ProgramError::InvalidAccountData);
        }

        let shuttle_owner = shuttle_owner_opt
            .as_ref()
            .ok_or(ProgramError::InvalidAccountData)?;
        let (mint, shuttle_ephemeral_amount) = {
            let shuttle_ephemeral_ata_data = shuttle_ephemeral_ata_info.try_borrow()?;
            let (ephemeral_owner, mint, amount) =
                read_ephemeral_ata_compat(&shuttle_ephemeral_ata_data).map_err(|err| {
                    if err == ProgramError::UninitializedAccount {
                        ProgramError::InvalidAccountData
                    } else {
                        err
                    }
                })?;
            if ephemeral_owner != *shuttle_info.address() {
                return Err(ProgramError::InvalidAccountData);
            }
            (mint, amount)
        };

        if shuttle_ephemeral_amount != 0 {
            if mint != *mint_info.address() {
                return Err(ProgramError::InvalidAccountData);
            }

            withdraw_ephemeral_ata_tokens(
                shuttle_info,
                false,
                shuttle_ephemeral_ata_info,
                vault_info,
                mint_info,
                vault_source_token_acc,
                destination_token_info,
                token_program_info,
                shuttle_ephemeral_amount,
            )?;
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

        close_program_account_to_recipient(shuttle_ephemeral_ata_info, rent_reimbursement_info)?;
    }

    if shuttle_present {
        close_program_account_to_recipient(shuttle_info, rent_reimbursement_info)?;
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
