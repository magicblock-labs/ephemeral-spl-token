use crate::processor::internal::token_vault::withdraw_ephemeral_ata_tokens;
use crate::processor::utils::validate_token_account;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{
    ephemeral_ata::read_ephemeral_ata_compat, load_initialized,
    shuttle_ephemeral_ata::ShuttleMetadata,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts, require_some};
use pinocchio::cpi::Signer;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::instructions::CloseAccount;
const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - Any     : Shuttle rent reimbursement account (must equal `ShuttleMetadata.payer`).
///  1: [writable]          - PDA     : Shuttle metadata account.
///  2: [writable]          - PDA     : Shuttle EATA account (PDA derived from [shuttle_metadata, mint]).
///  3: [writable]          - SPL     : Shuttle wallet ATA account.
///  4: [writable]          - SPL     : Destination token account.
///  5: []                  - SPL     : Mint account.
///  6: []                  - PDA     : Global vault account.
///  7: [writable]          - SPL     : Vault source token account.
///  8: []                  - SPL     : Token program account.
///  9: []                  - Program : Source program (must equal this program).
/// 10: []                  - Any     : Escrow authority.
/// 11: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: escrow_index (u8)
///
pub fn process_close_shuttle_ata_intent(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        rent_reimbursement_info, // force multi-line
        shuttle_info,
        shuttle_ephemeral_ata_info,
        shuttle_wallet_ata_info,
        destination_token_info,
        mint_info,
        vault_info,
        vault_source_token_acc,
        token_program_info,
        source_program,
        escrow_authority,
        escrow_signer,
    ] = require_n_accounts!(accounts, 12);

    require!(
        instruction_data.len() == 1,
        ProgramError::InvalidInstructionData
    );
    let escrow_index = &instruction_data[0];

    require_eq_keys!(
        source_program.address(),
        &crate::ID,
        ProgramError::IncorrectAuthority
    );

    require!(
        escrow_signer.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    let escrow_index_seed = [*escrow_index];
    let (expected_escrow, _) = ephemeral_spl_api::Address::find_program_address(
        &[
            DLP_EPHEMERAL_BALANCE_TAG,
            escrow_authority.address().as_ref(),
            escrow_index_seed.as_ref(),
        ],
        &ephemeral_rollups_pinocchio::ID,
    );
    require_eq_keys!(
        &expected_escrow,
        escrow_signer.address(),
        ProgramError::InvalidSeeds
    );

    let shuttle_present = shuttle_info.lamports() > 0;
    let shuttle_ephemeral_present = shuttle_ephemeral_ata_info.lamports() > 0;
    let shuttle_wallet_present = shuttle_wallet_ata_info.lamports() > 0;

    let mut shuttle_id = 0u32;
    let mut shuttle_owner_opt = None;
    let mut shuttle_bump = None;
    if shuttle_present {
        require!(
            shuttle_info.owned_by(&crate::ID),
            ProgramError::InvalidAccountOwner
        );

        let shuttle =
            load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        require_eq_keys!(
            &shuttle.payer,
            rent_reimbursement_info.address(),
            ProgramError::IncorrectAuthority
        );
        shuttle_id = shuttle.id;
        let shuttle_owner = shuttle.owner;
        shuttle_owner_opt = Some(shuttle_owner);
        shuttle_bump = Some(shuttle.bump);
    }

    if shuttle_wallet_present {
        require!(
            shuttle_wallet_ata_info.owned_by(token_program_info.address()),
            ProgramError::InvalidAccountOwner
        );
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);

        let shuttle_owner =
            require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
        let (mint, shuttle_wallet_amount) = {
            let token_account = validate_token_account(
                shuttle_wallet_ata_info,
                mint_info.address(),
                Some(shuttle_info.address()),
                Some(token_program_info.address()),
            )?;
            (token_account.mint(), token_account.amount())
        };

        require!(shuttle_wallet_amount == 0, ProgramError::InvalidArgument);

        let shuttle_id_seed = shuttle_id.to_le_bytes();
        let derived_shuttle =
            ShuttleMetadata::derive_pda(shuttle_owner, mint, shuttle_id, shuttle_bump)?;
        require_eq_keys!(
            &derived_shuttle,
            shuttle_info.address(),
            ProgramError::InvalidSeeds
        );

        let bump = [shuttle_bump];
        let signer_seeds =
            ShuttleMetadata::signer_seeds(shuttle_owner, mint, &shuttle_id_seed, &bump);
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
        require!(
            shuttle_ephemeral_ata_info.owned_by(&crate::ID),
            ProgramError::InvalidAccountOwner
        );
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);

        let shuttle_owner =
            require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
        let (mint, shuttle_ephemeral_amount, shuttle_eata_bump) = {
            let shuttle_ephemeral_ata_data = shuttle_ephemeral_ata_info.try_borrow()?;
            let (ephemeral_owner, mint, amount, shuttle_eata_bump) =
                read_ephemeral_ata_compat(&shuttle_ephemeral_ata_data)?;
            require_eq_keys!(
                &ephemeral_owner,
                shuttle_info.address(),
                ProgramError::InvalidAccountData
            );
            (mint, amount, shuttle_eata_bump)
        };

        if shuttle_ephemeral_amount != 0 {
            require_eq_keys!(&mint, mint_info.address(), ProgramError::InvalidAccountData);

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

        let derived_shuttle =
            ShuttleMetadata::derive_pda(shuttle_owner, &mint, shuttle_id, shuttle_bump)?;

        require_eq_keys!(
            &derived_shuttle,
            shuttle_info.address(),
            ProgramError::InvalidSeeds
        );

        let derived_shuttle_ephemeral_ata =
            EphemeralAta::derive_pda(shuttle_info.address(), &mint, shuttle_eata_bump)?;
        require_eq_keys!(
            &derived_shuttle_ephemeral_ata,
            shuttle_ephemeral_ata_info.address(),
            ProgramError::InvalidSeeds
        );

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
    require!(
        recipient.address() != account.address(),
        ProgramError::InvalidArgument
    );

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
