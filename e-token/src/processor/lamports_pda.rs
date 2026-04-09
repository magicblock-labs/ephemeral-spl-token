use dlp_api::compact::ClearText;
use dlp_api::{requires::require_initialized_delegation_record, state::DelegationRecord};
use ephemeral_rollups_pinocchio::{
    consts::{DELEGATION_PROGRAM_ID, MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    intent_bundle::{ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta},
    types::DelegateConfig,
};
use pinocchio::cpi::{Seed, Signer};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::{CreateAccount, Transfer};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::{
    assert_owner, assert_signer,
    processor::{
        deposit_and_delegate_shuttle_ephemeral_ata_with_merge::delegate_account_with_actions_from_sponsor,
        rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    },
};

const DLP_EPHEMERAL_BALANCE_TAG: &[u8] = b"balance";
pub(crate) const LAMPORTS_PDA_SEED: &[u8] = b"lamports";
const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 512;
const CLOSE_LAMPORTS_PDA_COMPUTE_UNITS: u32 = 50_000;

#[inline(never)]
pub fn process_sponsored_lamports_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let (amount, salt) = parse_amount_and_salt(instruction_data)?;
    let [payer_info, rent_pda_info, lamports_pda_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, destination_info, destination_delegation_record_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if amount == 0 {
        return Err(ProgramError::InvalidArgument);
    }
    assert_signer!(payer_info);
    assert_owner!(rent_pda_info, &pinocchio_system::ID);
    assert_owner!(destination_info, &DELEGATION_PROGRAM_ID);

    if owner_program.address() != &ephemeral_spl_api::program::id_address() {
        return Err(ProgramError::IncorrectProgramId);
    }

    if RENT_PDA != *rent_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if rent_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let validator =
        read_destination_validator(destination_info, destination_delegation_record_info)?;
    let (derived_lamports_pda, lamports_pda_bump) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    if derived_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if lamports_pda_info.lamports() > 0 {
        return Err(ProgramError::InvalidAccountData);
    }

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
        owner: &ephemeral_spl_api::program::id_address(),
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

#[inline(never)]
pub fn process_transfer_lamports_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let (amount, salt) = parse_amount_and_salt(instruction_data)?;
    let [payer_info, lamports_pda_info, destination_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);
    assert_owner!(lamports_pda_info, &ephemeral_spl_api::program::id_address());

    if lamports_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    if derived_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let expected_balance = Rent::get()?
        .try_minimum_balance(0)?
        .checked_add(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    if lamports_pda_info.lamports() < expected_balance {
        return Err(ProgramError::InvalidArgument);
    }

    transfer_lamports(lamports_pda_info, destination_info, amount)
}

#[inline(never)]
pub fn process_undelegate_lamports_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let salt = parse_salt(instruction_data)?;
    let [payer_info, rent_pda_info, lamports_pda_info, destination_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);
    assert_owner!(lamports_pda_info, &ephemeral_spl_api::program::id_address());

    if lamports_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    if derived_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if lamports_pda_info.lamports() < Rent::get()?.try_minimum_balance(0)? {
        return Err(ProgramError::InvalidArgument);
    }

    let close_handler_data = close_lamports_handler_data(DEFAULT_ESCROW_INDEX, &salt);
    let close_handler_accounts = [
        ShortAccountMeta {
            pubkey: *rent_pda_info.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *lamports_pda_info.address(),
            is_writable: true,
        },
        ShortAccountMeta {
            pubkey: *payer_info.address(),
            is_writable: false,
        },
        ShortAccountMeta {
            pubkey: *destination_info.address(),
            is_writable: false,
        },
    ];
    let close_handler = [CallHandler {
        destination_program: Address::new_from_array(crate::ID),
        escrow_authority: payer_info.clone(),
        args: ActionArgs::new(&close_handler_data).with_escrow_index(DEFAULT_ESCROW_INDEX),
        compute_units: CLOSE_LAMPORTS_PDA_COMPUTE_UNITS,
        accounts: &close_handler_accounts,
        callback: None,
    }];
    let committed_accounts = [lamports_pda_info.clone()];
    let mut intent_bundle_data = [0u8; INTENT_BUNDLE_DATA_BUF_SIZE];

    MagicIntentBundleBuilder::new(
        payer_info.clone(),
        magic_context.clone(),
        magic_program.clone(),
    )
    .commit_and_undelegate(&committed_accounts)
    .add_post_undelegate_actions(&close_handler)
    .build_and_invoke(&mut intent_bundle_data)
}

#[inline(never)]
pub fn process_close_lamports_pda_intent(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let (escrow_index, salt) = parse_escrow_index_and_salt(instruction_data)?;
    if accounts.len() < 6 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let rent_pda_info = &accounts[0];
    let lamports_pda_info = &accounts[1];
    let payer_info = &accounts[2];
    let destination_info = &accounts[3];
    let escrow_authority = &accounts[accounts.len() - 2];
    let escrow_signer = &accounts[accounts.len() - 1];

    assert_signer!(escrow_signer);
    assert_owner!(rent_pda_info, &pinocchio_system::ID);
    assert_owner!(lamports_pda_info, &ephemeral_spl_api::program::id_address());

    let escrow_index_seed = [escrow_index];
    let (expected_escrow, _) = Address::find_program_address(
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

    if RENT_PDA != *rent_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if rent_pda_info.data_len() != 0 || lamports_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let (derived_lamports_pda, _) =
        derive_lamports_pda(payer_info.address(), destination_info.address(), &salt);
    if derived_lamports_pda != *lamports_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    if lamports_pda_info.lamports() < Rent::get()?.try_minimum_balance(0)? {
        return Err(ProgramError::InvalidArgument);
    }

    close_program_account_to_recipient(lamports_pda_info, rent_pda_info)
}

pub(crate) fn derive_lamports_pda(
    payer: &Address,
    destination: &Address,
    salt: &[u8; 32],
) -> (Address, u8) {
    Address::find_program_address(
        &[
            LAMPORTS_PDA_SEED,
            payer.as_ref(),
            destination.as_ref(),
            salt.as_ref(),
        ],
        &ephemeral_spl_api::program::id_address(),
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
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
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
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
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

fn close_lamports_handler_data(escrow_index: u8, salt: &[u8; 32]) -> [u8; 34] {
    let mut data = [0u8; 34];
    data[0] = ephemeral_spl_api::instruction::internal::CLOSE_LAMPORTS_PDA_INTENT;
    data[1] = escrow_index;
    data[2..].copy_from_slice(salt);
    data
}

#[allow(clippy::too_many_arguments)]
fn parse_amount_and_salt(instruction_data: &[u8]) -> Result<(u64, [u8; 32]), ProgramError> {
    if instruction_data.len() != 40 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut amount = [0u8; 8];
    amount.copy_from_slice(&instruction_data[..8]);

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&instruction_data[8..40]);

    Ok((u64::from_le_bytes(amount), salt))
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

fn parse_salt(instruction_data: &[u8]) -> Result<[u8; 32], ProgramError> {
    if instruction_data.len() != 32 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut salt = [0u8; 32];
    salt.copy_from_slice(instruction_data);
    Ok(salt)
}

fn parse_escrow_index_and_salt(instruction_data: &[u8]) -> Result<(u8, [u8; 32]), ProgramError> {
    if instruction_data.len() != 33 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&instruction_data[1..]);
    Ok((instruction_data[0], salt))
}

fn transfer_lamports(
    source: &AccountView,
    destination: &AccountView,
    amount: u64,
) -> ProgramResult {
    if *source.address() == *destination.address() {
        return Err(ProgramError::InvalidArgument);
    }

    let updated_source_lamports = source
        .lamports()
        .checked_sub(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    let updated_destination_lamports = destination
        .lamports()
        .checked_add(amount)
        .ok_or(ProgramError::InvalidArgument)?;

    source.set_lamports(updated_source_lamports);
    destination.set_lamports(updated_destination_lamports);
    Ok(())
}

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
