use ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer;
use ephemeral_spl_api::{
    instructions::ExecuteQueuedTransferArgs, require, require_eq_keys, require_n_accounts,
    state::global_vault::GlobalVault,
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;
use wheels::layout::Decodable as _;

use crate::processor::internal::{
    read_mint_decimals,
    rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    token_vault::validate_vault_for_mint,
};

///
/// Executes on: BASE only.
///
/// Accounts:
///
///  0: []                  - PDA     : Global vault account.
///  1: []                  - SPL     : Mint account.
///  2: [writable]          - SPL     : Vault token account.
///  3: []                  - Any     : Destination owner.
///  4: [writable]          - SPL     : Destination token account.
///  5: [writable]          - PDA     : Global rent PDA.
///  6: []                  - SPL     : Token program.
///  7: []                  - SPL     : Associated token program.
///  8: []                  - Builtin : System program.
///  9: []                  - Program : Source program (must equal this program).
/// 10: []                  - PDA     : Queue PDA authority.
/// 11: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: ExecuteQueuedTransferArgs
///
#[inline(always)]
pub fn process_execute_ready_queued_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        vault_info, // force multi-line
        mint_info,
        vault_token_acc_info,
        destination_owner_info,
        destination_token_acc_info,
        rent_pda_info,
        token_program_info,
        associated_token_program_info,
        system_program_info,
        source_program,
        escrow_authority,
        escrow_signer,
    ] = require_n_accounts!(accounts, 12);

    let args = ExecuteQueuedTransferArgs::decode(instruction_data)?;

    // Note that accounts [source_program, escrow_authority, escrow_signer] are appended by DLP's
    // CallHandlerV2 instruction.
    require_eq_keys!(
        source_program.address(),
        &crate::ID,
        ProgramError::IncorrectAuthority
    );

    require!(
        escrow_signer.is_signer(),
        ProgramError::MissingRequiredSignature
    );

    let expected_escrow =
        ephemeral_balance_pda_from_payer(escrow_authority.address(), args.escrow_index());
    require_eq_keys!(
        &expected_escrow,
        escrow_signer.address(),
        ProgramError::InvalidSeeds
    );

    if args.should_create_destination_ata_idempotent() {
        require!(
            rent_pda_info.owned_by(&SYSTEM_PROGRAM_ID),
            ProgramError::InvalidAccountOwner
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
        require!(
            associated_token_program_info.address() == &pinocchio_associated_token_account::ID
                && system_program_info.address() == &SYSTEM_PROGRAM_ID,
            ProgramError::InvalidAccountData
        );

        let rent_bump_seed = [RENT_PDA_BUMP];
        let rent_signer_seed = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
        let rent_signer = Signer::from(&rent_signer_seed);

        (pinocchio_associated_token_account::instructions::CreateIdempotent {
            funding_account: rent_pda_info,
            account: destination_token_acc_info,
            wallet: destination_owner_info,
            mint: mint_info,
            system_program: system_program_info,
            token_program: token_program_info,
        })
        .invoke_signed(&[rent_signer])?;
    }

    let vault_bump = validate_vault_for_mint(vault_info, mint_info, vault_token_acc_info)?;
    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let vault_bump = [vault_bump];
    let signer_seeds = GlobalVault::signer_seeds(mint_info.address(), &vault_bump);
    let signer = Signer::from(&signer_seeds);

    pinocchio_token_2022::instructions::TransferChecked {
        mint: mint_info,
        from: vault_token_acc_info,
        to: destination_token_acc_info,
        authority: vault_info,
        token_program: token_program_info.address(),
        amount: args.amount(),
        decimals,
    }
    .invoke_signed(&[signer])?;

    if let Some(client_ref_id) = args.client_ref_id() {
        if client_ref_id != 0 {
            pinocchio_log::log!("client_ref_id: {}", client_ref_id);
        }
    }

    Ok(())
}
