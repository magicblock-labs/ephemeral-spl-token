use dlp_api::{
    compact::ClearText, requires::require_initialized_delegation_record, state::DelegationRecord,
};
use ephemeral_rollups_pinocchio::{
    consts::{DELEGATION_PROGRAM_ID, MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    types::DelegateConfig,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::{CreateAccount, Transfer};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::processor::{
    initialize_rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    internal::lamports_pda::{derive_lamports_pda, parse_amount_and_salt, LAMPORTS_PDA_SEED},
    internal::shuttle_delegation::delegate_account_with_actions_from_sponsor,
};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Lamports PDA account.
///  3: []                  - Program : Owner program.
///  4: [writable]          - PDA     : Buffer account.
///  5: [writable]          - PDA     : Delegation record account.
///  6: [writable]          - PDA     : Delegation metadata account.
///  7: []                  - Program : Delegation program.
///  8: []                  - Builtin : System program.
///  9: [writable]          - PDA     : Destination account.
/// 10: [writable]          - PDA     : Destination delegation record account.
///
/// Instruction Data: amount (u64) + salt ([u8; 32])
///
#[inline(never)]
pub fn process_sponsored_lamports_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        lamports_pda_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
        destination_info,
        destination_delegation_record_info,
    ] = require_n_accounts!(accounts, 11);

    let (amount, salt) = parse_amount_and_salt(instruction_data)?;

    require!(amount != 0, ProgramError::InvalidArgument);
    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        rent_pda_info.owned_by(&pinocchio_system::ID),
        ProgramError::InvalidAccountOwner
    );
    require!(
        destination_info.owned_by(&DELEGATION_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );

    require_eq_keys!(
        owner_program.address(),
        &crate::ID,
        ProgramError::IncorrectProgramId
    );

    require_eq_keys!(
        &RENT_PDA,
        rent_pda_info.address(),
        ProgramError::InvalidSeeds
    );
    require!(
        rent_pda_info.data_len() == 0,
        ProgramError::InvalidAccountData
    );

    let validator =
        read_destination_validator(destination_info, destination_delegation_record_info)?;
    let (derived_lamports_pda, lamports_pda_bump) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    require_eq_keys!(
        &derived_lamports_pda,
        lamports_pda_info.address(),
        ProgramError::InvalidSeeds
    );
    require!(
        lamports_pda_info.lamports() == 0,
        ProgramError::InvalidAccountData
    );

    Transfer {
        from: payer_info,
        to: rent_pda_info,
        lamports: ephemeral_spl_api::consts::SPONSORED_LAMPORTS_TRANSFER_SETUP_LAMPORTS,
    }
    .invoke()?;

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seeds);

    let lamports_pda_bump_seed = [lamports_pda_bump];
    let lamports_pda_signer_seeds = [
        Seed::from(LAMPORTS_PDA_SEED),
        Seed::from(payer_info.address().as_ref()),
        Seed::from(destination_info.address().as_ref()),
        Seed::from(salt.as_ref()),
        Seed::from(&lamports_pda_bump_seed),
    ];
    let lamports_pda_signer = Signer::from(&lamports_pda_signer_seeds);

    CreateAccount {
        from: rent_pda_info,
        to: lamports_pda_info,
        space: 0,
        lamports: Rent::get()?.try_minimum_balance(0)?,
        owner: &crate::ID,
    }
    .invoke_signed(&[rent_signer.clone(), lamports_pda_signer])?;

    Transfer {
        from: payer_info,
        to: lamports_pda_info,
        lamports: amount,
    }
    .invoke()?;

    let pda_seeds = [
        LAMPORTS_PDA_SEED,
        payer_info.address().as_ref(),
        destination_info.address().as_ref(),
        salt.as_ref(),
    ];
    let post_actions = alloc::vec![
        transfer_lamports_pda_action(
            payer_info,
            lamports_pda_info,
            destination_info,
            amount,
            &salt,
        ),
        undelegate_lamports_pda_action(
            payer_info,
            rent_pda_info,
            lamports_pda_info,
            destination_info,
            &salt,
        ),
    ];

    delegate_account_with_actions_from_sponsor(
        rent_pda_info,
        rent_signer,
        lamports_pda_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
        &pda_seeds,
        lamports_pda_bump,
        DelegateConfig {
            validator: Some(validator),
            ..DelegateConfig::default()
        },
        post_actions.cleartext(),
        &[payer_info],
    )
}

fn transfer_lamports_pda_action(
    payer_info: &AccountView,
    lamports_pda_info: &AccountView,
    destination_info: &AccountView,
    amount: u64,
    salt: &[u8; 32],
) -> Instruction {
    let mut data = alloc::vec![ephemeral_spl_api::instruction::internal::TRANSFER_LAMPORTS_PDA];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(salt);

    Instruction {
        program_id: Pubkey::from(crate::ID),
        accounts: alloc::vec![
            AccountMeta::new_readonly(*payer_info.address(), true),
            AccountMeta::new(*lamports_pda_info.address(), false),
            AccountMeta::new(*destination_info.address(), false),
        ],
        data,
    }
}

fn undelegate_lamports_pda_action(
    payer_info: &AccountView,
    rent_pda_info: &AccountView,
    lamports_pda_info: &AccountView,
    destination_info: &AccountView,
    salt: &[u8; 32],
) -> Instruction {
    let mut data = alloc::vec![ephemeral_spl_api::instruction::internal::UNDELEGATE_LAMPORTS_PDA];
    data.extend_from_slice(salt);

    Instruction {
        program_id: Pubkey::from(crate::ID),
        accounts: alloc::vec![
            AccountMeta::new_readonly(*payer_info.address(), true),
            AccountMeta::new_readonly(*rent_pda_info.address(), false),
            AccountMeta::new(*lamports_pda_info.address(), false),
            AccountMeta::new_readonly(*destination_info.address(), false),
            AccountMeta::new(Pubkey::from(MAGIC_CONTEXT_ID.to_bytes()), false),
            AccountMeta::new_readonly(Pubkey::from(MAGIC_PROGRAM_ID.to_bytes()), false),
        ],
        data,
    }
}

fn read_destination_validator(
    destination_info: &AccountView,
    destination_delegation_record_info: &AccountView,
) -> Result<Address, ProgramError> {
    require_initialized_delegation_record(
        destination_info,
        destination_delegation_record_info,
        false,
    )?;

    let destination_delegation_record_data = destination_delegation_record_info.try_borrow()?;
    let destination_delegation_record =
        DelegationRecord::try_from_bytes_with_discriminator(&destination_delegation_record_data)
            .map_err(|_| ProgramError::InvalidAccountData)?;

    Ok(Address::new_from_array(
        destination_delegation_record.authority.to_bytes(),
    ))
}
