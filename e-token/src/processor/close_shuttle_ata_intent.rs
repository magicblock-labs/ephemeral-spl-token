use ephemeral_spl_api::{
    require, require_eq_keys, require_n_accounts, require_some,
    state::{
        ephemeral_ata::{read_ephemeral_ata_compat, EphemeralAta},
        load_initialized,
        shuttle_ephemeral_ata::ShuttleMetadata,
        stash::StashPda,
    },
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_system::{instructions::Transfer, ID as SYSTEM_PROGRAM_ID};
use pinocchio_token_2022::instructions::{CloseAccount, TransferChecked};
use solana_address::{address_eq, Address};
use spl_token_interface::ID as SPL_TOKEN_PROGRAM_ID;

use crate::processor::internal::{
    get_associated_token_address, read_mint_decimals,
    rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    token_vault::withdraw_ephemeral_ata_tokens,
    unwrap_pda::NATIVE_MINT,
    validate_token_account,
};
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
///  9: [writable]          - Any     : Owner wallet (native SOL only).
/// 10: []                  - Program : System program (native SOL only).
///
/// The following accounts are appended by DLP after the action accounts:
///
///  *: []                  - Program : Source program (must equal this program).
///  *: []                  - Any     : Escrow authority.
///  *: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: escrow_index (u8), optionally followed by
/// `[user(32) | stash_bump(1)]` for the stash close path. In that path the
/// escrow authority is the stash PDA and account 0 is the rent sink.
///
pub fn process_close_shuttle_ata_intent(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let (head_accounts, native_accounts, source_program, escrow_authority, escrow_signer) = match accounts.len() {
        12 => (&accounts[..9], None, &accounts[9], &accounts[10], &accounts[11]),
        14 => (
            &accounts[..9],
            Some(&accounts[9..11]),
            &accounts[11],
            &accounts[12],
            &accounts[13],
        ),
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };

    let (escrow_index, close_stash_seeds) = parse_close_shuttle_intent_data(instruction_data)?;

    require_eq_keys!(source_program.address(), &crate::ID, ProgramError::IncorrectAuthority);

    require!(escrow_signer.is_signer(), ProgramError::MissingRequiredSignature);

    let escrow_index_seed = [escrow_index];
    let (expected_escrow, _) = Address::find_program_address(
        &[
            DLP_EPHEMERAL_BALANCE_TAG,
            escrow_authority.address().as_ref(),
            escrow_index_seed.as_ref(),
        ],
        &ephemeral_rollups_pinocchio::ID,
    );
    require_eq_keys!(&expected_escrow, escrow_signer.address(), ProgramError::InvalidSeeds);

    settle_and_close_shuttle_accounts(
        head_accounts,
        close_stash_seeds.map(|(user, stash_bump)| (escrow_authority, user, stash_bump)),
        native_accounts,
    )
}

fn parse_close_shuttle_intent_data(instruction_data: &[u8]) -> Result<(u8, Option<([u8; 32], u8)>), ProgramError> {
    match instruction_data.len() {
        1 => Ok((instruction_data[0], None)),
        n if n == 1 + CLOSE_STASH_DATA_LEN => {
            let mut user = [0u8; 32];
            user.copy_from_slice(&instruction_data[1..33]);
            Ok((instruction_data[0], Some((user, instruction_data[33]))))
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

pub(crate) fn settle_and_close_shuttle_accounts(
    head_accounts: &[AccountView],
    close_stash: Option<(&AccountView, [u8; 32], u8)>,
    native_accounts: Option<&[AccountView]>,
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
    ] = require_n_accounts!(head_accounts, 9);

    let shuttle_present = shuttle_info.lamports() > 0;
    let shuttle_ephemeral_present = shuttle_ephemeral_ata_info.lamports() > 0;
    let shuttle_wallet_present = shuttle_wallet_ata_info.lamports() > 0;

    let mut shuttle_id = 0u32;
    let mut shuttle_owner_opt = None;
    let mut shuttle_bump = None;
    if shuttle_present {
        require!(shuttle_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);

        let shuttle = load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
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

    let mut shuttle_wallet_amount = 0;
    if shuttle_wallet_present {
        require!(
            shuttle_wallet_ata_info.owned_by(token_program_info.address()),
            ProgramError::InvalidAccountOwner
        );
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);

        let shuttle_owner = require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
        let mint = {
            let token_account = validate_token_account(
                shuttle_wallet_ata_info,
                mint_info.address(),
                Some(shuttle_info.address()),
                Some(token_program_info.address()),
            )?;
            shuttle_wallet_amount = token_account.amount();
            *token_account.mint()
        };

        let derived_shuttle = ShuttleMetadata::derive_pda(shuttle_owner, &mint, shuttle_id, shuttle_bump)?;
        require_eq_keys!(&derived_shuttle, shuttle_info.address(), ProgramError::InvalidSeeds);
    }

    let mut shuttle_ephemeral_amount = 0;
    if shuttle_ephemeral_present {
        require!(
            shuttle_ephemeral_ata_info.owned_by(&crate::ID),
            ProgramError::InvalidAccountOwner
        );
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);

        let shuttle_owner = require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
        let (mint, amount, eata_bump) = {
            let shuttle_ephemeral_ata_data = shuttle_ephemeral_ata_info.try_borrow()?;
            let (ephemeral_owner, mint, amount, eata_bump) = read_ephemeral_ata_compat(&shuttle_ephemeral_ata_data)?;
            require_eq_keys!(
                &ephemeral_owner,
                shuttle_info.address(),
                ProgramError::InvalidAccountData
            );
            (mint, amount, eata_bump)
        };
        require_eq_keys!(&mint, mint_info.address(), ProgramError::InvalidAccountData);

        let derived_shuttle = ShuttleMetadata::derive_pda(shuttle_owner, &mint, shuttle_id, shuttle_bump)?;
        require_eq_keys!(&derived_shuttle, shuttle_info.address(), ProgramError::InvalidSeeds);

        let derived_shuttle_ephemeral_ata = EphemeralAta::derive_pda(shuttle_info.address(), &mint, eata_bump)?;
        require_eq_keys!(
            &derived_shuttle_ephemeral_ata,
            shuttle_ephemeral_ata_info.address(),
            ProgramError::InvalidSeeds
        );

        shuttle_ephemeral_amount = amount;
    }

    let native_amount = shuttle_wallet_amount
        .checked_add(shuttle_ephemeral_amount)
        .ok_or(ProgramError::InvalidArgument)?;
    let deliver_native = native_delivery_eligible(
        native_accounts,
        rent_reimbursement_info,
        shuttle_owner_opt.as_ref(),
        mint_info,
        token_program_info,
        shuttle_wallet_present,
        native_amount,
    )?;

    if shuttle_ephemeral_amount != 0 {
        withdraw_ephemeral_ata_tokens(
            shuttle_info,
            false,
            shuttle_ephemeral_ata_info,
            vault_info,
            mint_info,
            vault_source_token_acc,
            if deliver_native {
                shuttle_wallet_ata_info
            } else {
                destination_token_info
            },
            token_program_info,
            shuttle_ephemeral_amount,
        )?;
    }

    if shuttle_wallet_present {
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);
        let shuttle_owner = require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
        let shuttle_id_seed = shuttle_id.to_le_bytes();
        let bump = [shuttle_bump];
        let signer_seeds = ShuttleMetadata::signer_seeds(shuttle_owner, mint_info.address(), &shuttle_id_seed, &bump);
        let signer = Signer::from(&signer_seeds);

        if shuttle_wallet_amount != 0 && !deliver_native {
            let decimals = read_mint_decimals(mint_info, token_program_info)?;
            TransferChecked {
                mint: mint_info,
                from: shuttle_wallet_ata_info,
                to: destination_token_info,
                authority: shuttle_info,
                token_program: token_program_info.address(),
                amount: shuttle_wallet_amount,
                decimals,
            }
            .invoke_signed(core::slice::from_ref(&signer))?;
        }

        CloseAccount {
            account: shuttle_wallet_ata_info,
            destination: rent_reimbursement_info,
            authority: shuttle_info,
            token_program: token_program_info.address(),
        }
        .invoke_signed(&[signer])?;
    }

    if deliver_native {
        transfer_native_to_owner(
            native_accounts.ok_or(ProgramError::NotEnoughAccountKeys)?,
            rent_reimbursement_info,
            native_amount,
        )?;
    }

    if let Some((stash_pda_info, user, stash_bump)) = close_stash {
        close_empty_stash_after_settlement(
            stash_pda_info,
            rent_reimbursement_info,
            destination_token_info,
            mint_info,
            token_program_info,
            &user,
            stash_bump,
        )?;
    }

    // Keep direct lamport/account closes last; the stash close path still needs
    // token/system CPIs, and those must run before these local lamport edits.
    if shuttle_ephemeral_present {
        close_program_account_to_recipient(shuttle_ephemeral_ata_info, rent_reimbursement_info)?;
    }

    if shuttle_present {
        close_program_account_to_recipient(shuttle_info, rent_reimbursement_info)?;
    }

    Ok(())
}

#[inline(always)]
fn native_delivery_eligible(
    native_accounts: Option<&[AccountView]>,
    rent_reimbursement_info: &AccountView,
    shuttle_owner: Option<&Address>,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    shuttle_wallet_present: bool,
    amount: u64,
) -> Result<bool, ProgramError> {
    let Some(native_accounts) = native_accounts else {
        return Ok(false);
    };
    let [owner_wallet_info, system_program_info] = require_n_accounts!(native_accounts, 2);
    let Some(shuttle_owner) = shuttle_owner else {
        return Ok(false);
    };

    require_eq_keys!(
        system_program_info.address(),
        &SYSTEM_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(
        owner_wallet_info.address(),
        shuttle_owner,
        ProgramError::InvalidAccountData
    );

    if !shuttle_wallet_present
        || amount == 0
        || !address_eq(mint_info.address(), &NATIVE_MINT)
        || !address_eq(token_program_info.address(), &SPL_TOKEN_PROGRAM_ID)
        || rent_reimbursement_info.address() != &RENT_PDA
        || !rent_reimbursement_info.owned_by(&SYSTEM_PROGRAM_ID)
        || rent_reimbursement_info.data_len() != 0
        || !owner_wallet_info.owned_by(&SYSTEM_PROGRAM_ID)
        || owner_wallet_info.address() == rent_reimbursement_info.address()
    {
        return Ok(false);
    }

    if owner_wallet_info.lamports() == 0 && amount < Rent::get()?.try_minimum_balance(0)? {
        return Ok(false);
    }

    Ok(true)
}

#[inline(always)]
fn transfer_native_to_owner(
    native_accounts: &[AccountView],
    rent_pda_info: &AccountView,
    amount: u64,
) -> ProgramResult {
    let [owner_wallet_info, system_program_info] = require_n_accounts!(native_accounts, 2);
    require_eq_keys!(
        system_program_info.address(),
        &SYSTEM_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );
    require_eq_keys!(rent_pda_info.address(), &RENT_PDA, ProgramError::InvalidSeeds);

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seed = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seed);
    Transfer {
        from: rent_pda_info,
        to: owner_wallet_info,
        lamports: amount,
    }
    .invoke_signed(&[rent_signer])
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
    require_eq_keys!(rent_pda_info.address(), &RENT_PDA, ProgramError::InvalidSeeds);

    let user_address = Address::new_from_array(*user);

    let derived_stash_pda = StashPda::derive_pda(&user_address, mint_info.address(), stash_bump)?;
    require_eq_keys!(&derived_stash_pda, stash_pda_info.address(), ProgramError::InvalidSeeds);

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
    let stash_signer_seeds = StashPda::signer_seeds(&user_address, mint_info.address(), &bump_seed);
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
fn close_program_account_to_recipient(account: &AccountView, recipient: &AccountView) -> ProgramResult {
    require!(recipient.address() != account.address(), ProgramError::InvalidArgument);

    let lamports_to_refund = account.lamports();
    let updated_recipient_lamports = recipient
        .lamports()
        .checked_add(lamports_to_refund)
        .ok_or(ProgramError::InvalidArgument)?;
    recipient.set_lamports(updated_recipient_lamports);
    account.set_lamports(0);
    account.close()
}
