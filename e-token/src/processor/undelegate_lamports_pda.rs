use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::instruction::ESplInternalInstruction;
use crate::processor::internal::lamports_pda::derive_lamports_pda;

const DEFAULT_ESCROW_INDEX: u8 = u8::MAX;
const INTENT_BUNDLE_DATA_BUF_SIZE: usize = 512;
const CLOSE_LAMPORTS_PDA_COMPUTE_UNITS: u32 = 50_000;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Lamports PDA account.
///  3: [writable]          - Any     : Destination account.
///  4: [writable]          - Any     : Magic context account.
///  5: []                  - Program : Magic program.
///
/// Instruction Data: salt ([u8; 32])
///
#[inline(never)]
pub fn process_undelegate_lamports_pda(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        lamports_pda_info,
        destination_info,
        magic_context,
        magic_program,
    ] = require_n_accounts!(accounts, 6);

    let salt = parse_salt(instruction_data)?;

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        lamports_pda_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    require!(
        lamports_pda_info.data_len() == 0,
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
        destination_program: crate::ID,
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

fn close_lamports_handler_data(escrow_index: u8, salt: &[u8; 32]) -> alloc::vec::Vec<u8> {
    let mut payload = [0u8; 33];
    payload[0] = escrow_index;
    payload[1..].copy_from_slice(salt);
    ESplInternalInstruction::CloseLamportsPdaIntent.with_data(&payload)
}

fn parse_salt(instruction_data: &[u8]) -> Result<[u8; 32], ProgramError> {
    require!(
        instruction_data.len() == 32,
        ProgramError::InvalidInstructionData
    );

    let mut salt = [0u8; 32];
    salt.copy_from_slice(instruction_data);
    Ok(salt)
}
