use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::internal::token_vault::withdraw_ephemeral_ata_tokens;
use crate::processor::utils::{get_associated_token_address, validate_token_account};
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::state::{
    ephemeral_ata::read_ephemeral_ata_compat, load_initialized,
    shuttle_ephemeral_ata::ShuttleMetadata,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts, require_some};
use pinocchio::cpi::Signer;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::Transfer;
use pinocchio_token_2022::instructions::CloseAccount;
const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";

const CLOSE_STASH_DATA_LEN: usize = 33;

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
/// Optional trailing accounts (14-account variant, scheduled flow only):
/// 12: [writable]          - PDA     : Stash PDA (authority of `destination_token_info`).
/// 13: [writable]          - PDA     : Rent PDA (lamport sink for the closed stash).
///
/// Instruction Data: escrow_index (u8), optionally followed by
/// `[user(32) | stash_bump(1)]` for the stash close path.
///
pub fn process_close_shuttle_ata_intent(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let close_stash = match accounts.len() {
        12 => None,
        14 => Some((&accounts[12], &accounts[13])),
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };
    let head_accounts = &accounts[..12];
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
    ] = require_n_accounts!(head_accounts, 12);

    let (escrow_index, close_stash_seeds) = match (close_stash, instruction_data.len()) {
        (None, 1) => (&instruction_data[0], None),
        (Some(_), n) if n == 1 + CLOSE_STASH_DATA_LEN => (
            &instruction_data[0],
            Some((
                <&[u8; 32]>::try_from(&instruction_data[1..33])
                    .map_err(|_| ProgramError::InvalidInstructionData)?,
                instruction_data[33],
            )),
        ),
        _ => return Err(ProgramError::InvalidInstructionData),
    };

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

    if let (Some((stash_pda_info, rent_pda_info)), Some((user, stash_bump))) =
        (close_stash, close_stash_seeds)
    {
        close_empty_stash_after_settlement(
            stash_pda_info,
            rent_pda_info,
            destination_token_info,
            mint_info,
            token_program_info,
            user,
            stash_bump,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn close_empty_stash_after_settlement(
    stash_pda_info: &AccountView,
    rent_pda_info: &AccountView,
    destination_token_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    user: &[u8; 32],
    stash_bump: u8,
) -> ProgramResult {
    require_eq_keys!(
        rent_pda_info.address(),
        &RENT_PDA,
        ProgramError::InvalidSeeds
    );

    // SAFETY: `&[u8; 32]` and `&Address` share the same in-memory layout.
    let user_address: &Address = unsafe { &*(user.as_ptr() as *const Address) };

    let derived_stash_pda = StashPda::derive_pda(user_address, mint_info.address(), stash_bump)?;
    require_eq_keys!(
        &derived_stash_pda,
        stash_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    let expected_stash_ata = get_associated_token_address(
        stash_pda_info.address(),
        mint_info.address(),
        token_program_info.address(),
    );
    require_eq_keys!(
        &expected_stash_ata,
        destination_token_info.address(),
        ProgramError::InvalidSeeds
    );

    let token_account = validate_token_account(
        destination_token_info,
        mint_info.address(),
        Some(stash_pda_info.address()),
        Some(token_program_info.address()),
    )?;
    require!(token_account.amount() == 0, ProgramError::InvalidArgument);

    let bump_seed = [stash_bump];
    let stash_signer_seeds = StashPda::signer_seeds(user_address, mint_info.address(), &bump_seed);
    let stash_signer = Signer::from(&stash_signer_seeds);

    CloseAccount {
        account: destination_token_info,
        destination: rent_pda_info,
        authority: stash_pda_info,
        token_program: token_program_info.address(),
    }
    .invoke_signed(core::slice::from_ref(&stash_signer))?;

    let stash_lamports = stash_pda_info.lamports();
    if stash_lamports > 0 {
        Transfer {
            from: stash_pda_info,
            to: rent_pda_info,
            lamports: stash_lamports,
        }
        .invoke_signed(core::slice::from_ref(&stash_signer))?;
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
