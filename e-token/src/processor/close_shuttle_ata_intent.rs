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
use pinocchio_token_2022::instructions::CloseAccount;
use solana_address::{address_eq, Address};
use spl_token_interface::ID as SPL_TOKEN_PROGRAM_ID;

use crate::processor::internal::{
    get_associated_token_address,
    rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    token_vault::withdraw_ephemeral_ata_tokens,
    unwrap_pda::{NATIVE_MINT, UNWRAP_PDA, UNWRAP_PDA_BUMP, UNWRAP_PDA_SEED},
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
///  9: []                  - Program : Source program (must equal this program).
/// 10: []                  - Any     : Escrow authority.
/// 11: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: escrow_index (u8), optionally followed by
/// `[user(32) | stash_bump(1)]` for the stash close path. In that path the
/// escrow authority is the stash PDA and account 0 is the rent sink.
///
pub fn process_close_shuttle_ata_intent(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let (head_accounts, native_accounts, source_program, escrow_authority, escrow_signer) = match accounts.len() {
        12 => (&accounts[..9], None, &accounts[9], &accounts[10], &accounts[11]),
        18 => (
            &accounts[..9],
            Some(&accounts[9..15]),
            &accounts[15],
            &accounts[16],
            &accounts[17],
        ),
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };
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

    let (escrow_index, close_stash_seeds) = match instruction_data.len() {
        1 => (&instruction_data[0], None),
        n if n == 1 + CLOSE_STASH_DATA_LEN => (
            &instruction_data[0],
            Some((
                <&[u8; 32]>::try_from(&instruction_data[1..33]).map_err(|_| ProgramError::InvalidInstructionData)?,
                instruction_data[33],
            )),
        ),
        _ => return Err(ProgramError::InvalidInstructionData),
    };

    require_eq_keys!(source_program.address(), &crate::ID, ProgramError::IncorrectAuthority);

    require!(escrow_signer.is_signer(), ProgramError::MissingRequiredSignature);

    let escrow_index_seed = [*escrow_index];
    let (expected_escrow, _) = Address::find_program_address(
        &[
            DLP_EPHEMERAL_BALANCE_TAG,
            escrow_authority.address().as_ref(),
            escrow_index_seed.as_ref(),
        ],
        &ephemeral_rollups_pinocchio::ID,
    );
    require_eq_keys!(&expected_escrow, escrow_signer.address(), ProgramError::InvalidSeeds);

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

    if shuttle_wallet_present {
        require!(
            shuttle_wallet_ata_info.owned_by(token_program_info.address()),
            ProgramError::InvalidAccountOwner
        );
        let shuttle_bump = require_some!(shuttle_bump, ProgramError::InvalidAccountData);

        let shuttle_owner = require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
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
        let derived_shuttle = ShuttleMetadata::derive_pda(shuttle_owner, mint, shuttle_id, shuttle_bump)?;
        require_eq_keys!(&derived_shuttle, shuttle_info.address(), ProgramError::InvalidSeeds);

        let bump = [shuttle_bump];
        let signer_seeds = ShuttleMetadata::signer_seeds(shuttle_owner, mint, &shuttle_id_seed, &bump);
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

        let shuttle_owner = require_some!(shuttle_owner_opt.as_ref(), ProgramError::InvalidAccountData);
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

            match native_accounts {
                Some(native_accounts)
                    if shuttle_native_delivery_eligible(
                        native_accounts,
                        mint_info,
                        token_program_info,
                        shuttle_ephemeral_amount,
                    )? =>
                {
                    deliver_shuttle_native(
                        native_accounts,
                        shuttle_owner,
                        shuttle_info,
                        shuttle_ephemeral_ata_info,
                        vault_info,
                        mint_info,
                        vault_source_token_acc,
                        token_program_info,
                        shuttle_ephemeral_amount,
                    )?;
                }
                _ => {
                    if native_accounts.is_some() {
                        // Owner wallet or mint state no longer supports a native payout;
                        // deliver the wrapped token to the owner ATA instead of failing.
                        pinocchio_log::log!("deliver_native: falling back to token delivery");
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
            }
        }

        let derived_shuttle = ShuttleMetadata::derive_pda(shuttle_owner, &mint, shuttle_id, shuttle_bump)?;

        require_eq_keys!(&derived_shuttle, shuttle_info.address(), ProgramError::InvalidSeeds);

        let derived_shuttle_ephemeral_ata = EphemeralAta::derive_pda(shuttle_info.address(), &mint, shuttle_eata_bump)?;
        require_eq_keys!(
            &derived_shuttle_ephemeral_ata,
            shuttle_ephemeral_ata_info.address(),
            ProgramError::InvalidSeeds
        );
    }

    if let Some((user, stash_bump)) = close_stash_seeds {
        close_empty_stash_after_settlement(
            escrow_authority,
            rent_reimbursement_info,
            destination_token_info,
            mint_info,
            token_program_info,
            user,
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

/// A native payout is only attempted when the mint, token program, and owner-wallet state all
/// support it; anything else degrades to the regular wrapped-token delivery so the withdrawal
/// still completes.
///
/// Unlike queued-transfer settlement (which hard-fails into the refund path so third-party
/// recipients never receive wrapped SOL), this close intent deliberately keeps the fallback:
/// it pays the withdrawing owner their own funds, and a failed close intent has no retry, so
/// degrading to the owner's wrapped-SOL ATA is strictly safer than stranding the balance.
/// Identity checks on program-derived accounts stay hard errors inside
/// `deliver_shuttle_native`.
#[inline(always)]
fn shuttle_native_delivery_eligible(
    native_accounts: &[AccountView],
    mint_info: &AccountView,
    token_program_info: &AccountView,
    amount: u64,
) -> Result<bool, ProgramError> {
    let owner_wallet_info = native_accounts.get(3).ok_or(ProgramError::NotEnoughAccountKeys)?;

    if !address_eq(mint_info.address(), &NATIVE_MINT)
        || !address_eq(token_program_info.address(), &SPL_TOKEN_PROGRAM_ID)
    {
        return Ok(false);
    }

    if !owner_wallet_info.owned_by(&SYSTEM_PROGRAM_ID) {
        return Ok(false);
    }

    // An unfunded wallet must end at or above the rent-exempt floor or the system transfer
    // would fail the whole close intent.
    if owner_wallet_info.lamports() == 0 && amount < Rent::get()?.try_minimum_balance(0)? {
        return Ok(false);
    }

    Ok(true)
}

/// Unwrap the shuttle's wrapped-SOL balance and pay the owner in native lamports.
///
/// The scratch WSOL account is created, filled with exactly `amount` (moved from the vault while
/// decrementing the shuttle ephemeral ATA), and closed within this instruction, so the rent PDA is
/// net-neutral and the owner receives exactly `amount` native lamports.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn deliver_shuttle_native(
    native_accounts: &[AccountView],
    owner: &Address,
    shuttle_info: &AccountView,
    shuttle_ephemeral_ata_info: &AccountView,
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_source_token_acc: &AccountView,
    token_program_info: &AccountView,
    amount: u64,
) -> ProgramResult {
    let [
        rent_pda_info, // force multi-line
        scratch_wsol_ata_info,
        unwrap_pda_info,
        owner_wallet_info,
        system_program_info,
        associated_token_program_info,
    ] = require_n_accounts!(native_accounts, 6);

    // Native unwrap only applies to the classic SPL Token wrapped-SOL mint.
    require_eq_keys!(mint_info.address(), &NATIVE_MINT, ProgramError::InvalidAccountData);
    require!(
        address_eq(token_program_info.address(), &SPL_TOKEN_PROGRAM_ID),
        ProgramError::IncorrectProgramId
    );

    // Recipient must be the shuttle owner's plain system wallet.
    require_eq_keys!(owner_wallet_info.address(), owner, ProgramError::InvalidAccountData);
    require!(
        owner_wallet_info.owned_by(&SYSTEM_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );

    // Rent PDA funds the scratch account and sinks the unwrapped lamports.
    require!(
        rent_pda_info.owned_by(&SYSTEM_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );
    require_eq_keys!(&RENT_PDA, rent_pda_info.address(), ProgramError::InvalidSeeds);
    require!(rent_pda_info.data_len() == 0, ProgramError::InvalidAccountData);
    require!(
        associated_token_program_info.address() == &pinocchio_associated_token_account::ID
            && system_program_info.address() == &SYSTEM_PROGRAM_ID,
        ProgramError::InvalidAccountData
    );

    // Scratch account must be the unwrap PDA's ATA for this mint.
    require_eq_keys!(unwrap_pda_info.address(), &UNWRAP_PDA, ProgramError::InvalidSeeds);
    let expected_scratch =
        get_associated_token_address(&UNWRAP_PDA, mint_info.address(), token_program_info.address());
    require_eq_keys!(
        &expected_scratch,
        scratch_wsol_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seed = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seed);

    // 1. Create the scratch WSOL account (funded by the rent PDA).
    (pinocchio_associated_token_account::instructions::CreateIdempotent {
        funding_account: rent_pda_info,
        account: scratch_wsol_ata_info,
        wallet: unwrap_pda_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    })
    .invoke_signed(&[rent_signer])?;

    // 2. Move exactly `amount` wrapped SOL from the vault into the scratch account and decrement
    //    the shuttle ephemeral ATA balance (vault-PDA signed inside the helper).
    withdraw_ephemeral_ata_tokens(
        shuttle_info,
        false,
        shuttle_ephemeral_ata_info,
        vault_info,
        mint_info,
        vault_source_token_acc,
        scratch_wsol_ata_info,
        token_program_info,
        amount,
    )?;

    // 3. Close the scratch account, unwrapping lamports (scratch rent + `amount`) to the rent PDA.
    let unwrap_bump_seed = [UNWRAP_PDA_BUMP];
    let unwrap_signer_seed = [Seed::from(UNWRAP_PDA_SEED), Seed::from(&unwrap_bump_seed)];
    let unwrap_signer = Signer::from(&unwrap_signer_seed);
    CloseAccount {
        account: scratch_wsol_ata_info,
        destination: rent_pda_info,
        authority: unwrap_pda_info,
        token_program: token_program_info.address(),
    }
    .invoke_signed(&[unwrap_signer])?;

    // 4. Forward exactly `amount` native lamports from the rent PDA to the owner wallet.
    let rent_signer = Signer::from(&rent_signer_seed);
    Transfer {
        from: rent_pda_info,
        to: owner_wallet_info,
        lamports: amount,
    }
    .invoke_signed(&[rent_signer])?;

    Ok(())
}
