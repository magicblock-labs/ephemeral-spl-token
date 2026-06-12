use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts_with_ignored};
use pinocchio::{
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};
use solana_address::Address;

use crate::processor::internal::{lamports_pda::derive_lamports_pda, rent_pda::RENT_PDA};

const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Rent PDA account.
///  1: [writable]          - PDA     : Lamports PDA account.
///  2: []                  - Any     : Payer / original authority used in PDA derivation.
///  3: []                  - Any     : Destination account.
///  4: []                  - Any     : Escrow authority.
///  5: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: escrow_index (u8) + salt ([u8; 32])
///
#[inline(never)]
pub fn process_close_lamports_pda_intent(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        rent_pda_info, // force multi-line
        lamports_pda_info,
        payer_info,
        destination_info,
    ] = require_n_accounts_with_ignored!(accounts, 4);

    let (escrow_index, salt) = parse_escrow_index_and_salt(instruction_data)?;
    require!(accounts.len() >= 6, ProgramError::NotEnoughAccountKeys);

    let escrow_authority = &accounts[accounts.len() - 2];
    let escrow_signer = &accounts[accounts.len() - 1];

    require!(
        escrow_signer.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        rent_pda_info.owned_by(&pinocchio_system::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(
        lamports_pda_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let escrow_index_seed = [escrow_index];
    let (expected_escrow, _) = Address::find_program_address(
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

    require_eq_keys!(
        &RENT_PDA,
        rent_pda_info.address(),
        ProgramError::InvalidSeeds
    );
    require!(
        rent_pda_info.data_len() == 0 && lamports_pda_info.data_len() == 0,
        ProgramError::InvalidAccountData
    );

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    require_eq_keys!(
        &derived_lamports_pda,
        lamports_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    require!(
        lamports_pda_info.lamports() >= Rent::get()?.try_minimum_balance(0)?,
        ProgramError::InvalidArgument
    );

    close_program_account_to_recipient(lamports_pda_info, rent_pda_info)
}

fn parse_escrow_index_and_salt(instruction_data: &[u8]) -> Result<(u8, [u8; 32]), ProgramError> {
    require!(
        instruction_data.len() == 33,
        ProgramError::InvalidInstructionData
    );

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&instruction_data[1..]);
    Ok((instruction_data[0], salt))
}

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
    account.close()
}
