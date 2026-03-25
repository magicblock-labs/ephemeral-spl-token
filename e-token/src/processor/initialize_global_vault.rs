use crate::processor::initialize_ephemeral_ata::process_initialize_ephemeral_ata;
use ephemeral_spl_api::state::RawType;
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::{CreateAccount, Transfer};
use {
    ephemeral_spl_api::state::global_vault::GlobalVault,
    ephemeral_spl_api::state::load_mut,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

const LEGACY_GLOBAL_VAULT_LEN: usize = core::mem::size_of::<pinocchio::Address>();
const GLOBAL_VAULT_V0_LEN: usize = 64;

#[inline(always)]
pub fn process_initialize_global_vault(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Global Vault account (PDA derived from [mint])
    // 1. [signer]   Payer (funds the account creation)
    // 2. []         Mint  (seed)
    // 3. [writable] Vault Ephemeral ATA account (PDA derived from [vault, mint])
    // 4. [writable] Vault associated token account
    // 5. []         Token program
    // 6. []         Associated token program
    // 7. []         System program

    let [vault_info, payer_info, mint_info, vault_ephemeral_ata_info, vault_token_acc_info, token_program_info, associated_token_program_info, system_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !pinocchio_associated_token_account::check_id(associated_token_program_info.address()) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let program_id = crate::ID;
    let (vault_derived_pda, vault_bump) = GlobalVault::find_pda(mint_info.address());
    if vault_derived_pda != *vault_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump = [vault_bump];
    let seed = GlobalVault::signer_seeds(mint_info.address(), &bump);
    let signer_seeds = Signer::from(&seed);
    let required_lamports = Rent::get()?.try_minimum_balance(GlobalVault::LEN)?;

    if vault_info.owned_by(&program_id) {
        let vault_data_len = vault_info.data_len();
        if vault_data_len == GlobalVault::LEN {
            // Already on current layout.
        } else if vault_data_len == LEGACY_GLOBAL_VAULT_LEN || vault_data_len == GLOBAL_VAULT_V0_LEN
        {
            // Migrate legacy vaults from 32-byte layout (mint only) to 64-byte layout.
            // TODO: Remove this migration path once all deployed vaults are upgraded.
            let legacy_mint = {
                let legacy_data = unsafe { vault_info.borrow_unchecked() };
                unsafe { *(legacy_data.as_ptr() as *const pinocchio::Address) }
            };
            if legacy_mint.ne(mint_info.address()) {
                return Err(ProgramError::InvalidAccountData);
            }

            let current_lamports = vault_info.lamports();
            if current_lamports < required_lamports {
                Transfer {
                    from: payer_info,
                    to: vault_info,
                    lamports: required_lamports - current_lamports,
                }
                .invoke()?;
            }

            vault_info.resize(GlobalVault::LEN)?;
        } else {
            return Err(ProgramError::InvalidAccountData);
        }
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

    let vault_eata_init_accounts = [
        vault_ephemeral_ata_info.clone(),
        payer_info.clone(),
        vault_info.clone(),
        mint_info.clone(),
    ];
    process_initialize_ephemeral_ata(&vault_eata_init_accounts, &[])?;

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
