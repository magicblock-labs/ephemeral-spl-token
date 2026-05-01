use crate::processor::internal::ephemeral_ata::initialize_ephemeral_ata_with_sponsor;
use ephemeral_spl_api::state::RawType;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::CreateAccount;
use {
    ephemeral_spl_api::state::global_vault::GlobalVault,
    ephemeral_spl_api::state::load_mut,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable]          - PDA     : Global vault account (PDA derived from [mint]).
///  1: [signer]            - Keypair : Payer.
///  2: []                  - SPL     : Mint.
///  3: [writable]          - PDA     : Vault Ephemeral ATA account (PDA derived from [vault, mint]).
///  4: [writable]          - SPL     : Vault associated token account.
///  5: []                  - SPL     : Token program.
///  6: []                  - SPL     : Associated token program.
///  7: []                  - Builtin : System program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_initialize_global_vault(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    let [
        vault_info, // force multi-line
        payer_info,
        mint_info,
        vault_ephemeral_ata_info,
        vault_token_acc_info,
        token_program_info,
        associated_token_program_info,
        system_program_info,
    ] = require_n_accounts!(accounts, 8);

    require!(
        pinocchio_associated_token_account::check_id(associated_token_program_info.address()),
        ProgramError::IncorrectProgramId
    );

    let program_id = crate::ID;
    let (vault_derived_pda, vault_bump) = GlobalVault::find_pda(mint_info.address());
    require_eq_keys!(
        &vault_derived_pda,
        vault_info.address(),
        ProgramError::InvalidSeeds
    );

    let bump = [vault_bump];
    let seed = GlobalVault::signer_seeds(mint_info.address(), &bump);
    let signer_seeds = Signer::from(&seed);
    let required_lamports = Rent::get()?.try_minimum_balance(GlobalVault::LEN)?;

    if vault_info.owned_by(&program_id) {
        require!(
            vault_info.data_len() == GlobalVault::LEN,
            ProgramError::InvalidAccountData
        );
    } else {
        CreateAccount {
            from: payer_info,
            to: vault_info,
            space: GlobalVault::LEN as u64,
            lamports: required_lamports,
            owner: &program_id,
        }
        .invoke_signed(&[signer_seeds])?;
    }

    initialize_ephemeral_ata_with_sponsor(
        vault_ephemeral_ata_info,
        payer_info,
        None,
        vault_info,
        mint_info,
    )?;

    pinocchio_associated_token_account::instructions::CreateIdempotent {
        funding_account: payer_info,
        account: vault_token_acc_info,
        wallet: vault_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    }
    .invoke()?;

    // Ensure account data has the expected size.
    let vault = load_mut::<GlobalVault>(unsafe { vault_info.borrow_unchecked_mut() })?;

    // Initialize the vault
    vault.mint = *mint_info.address();
    vault.token_account = *vault_token_acc_info.address();
    vault.bump = vault_bump;

    Ok(())
}
