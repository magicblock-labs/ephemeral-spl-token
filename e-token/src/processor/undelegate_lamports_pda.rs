use ephemeral_rollups_pinocchio::intent_bundle::{
    ActionArgs, CallHandler, MagicIntentBundleBuilder, ShortAccountMeta,
};
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use crate::{assert_owner, assert_signer, processor::internal::lamports_pda::derive_lamports_pda};

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
    let salt = parse_salt(instruction_data)?;
    let [payer_info, rent_pda_info, lamports_pda_info, destination_info, magic_context, magic_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    assert_signer!(payer_info);
    assert_owner!(lamports_pda_info, &crate::ID);

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
        destination_program: crate::ID,
        escrow_authority: payer_info.clone(),
        args: ActionArgs::new(&close_handler_data).with_escrow_index(DEFAULT_ESCROW_INDEX),
        compute_units: CLOSE_LAMPORTS_PDA_COMPUTE_UNITS,
        accounts: &close_handler_accounts,
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

fn close_lamports_handler_data(escrow_index: u8, salt: &[u8; 32]) -> [u8; 34] {
    let mut data = [0u8; 34];
    data[0] = ephemeral_spl_api::instruction::internal::CLOSE_LAMPORTS_PDA_INTENT;
    data[1] = escrow_index;
    data[2..].copy_from_slice(salt);
    data
}

fn parse_salt(instruction_data: &[u8]) -> Result<[u8; 32], ProgramError> {
    if instruction_data.len() != 32 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut salt = [0u8; 32];
    salt.copy_from_slice(instruction_data);
    Ok(salt)
}
