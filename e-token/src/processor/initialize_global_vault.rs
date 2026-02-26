use ephemeral_spl_api::state::RawType;
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::{CreateAccount, Transfer};
use {
    ephemeral_spl_api::state::global_vault::GlobalVault,
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

const LEGACY_GLOBAL_VAULT_LEN: usize = core::mem::size_of::<pinocchio::Address>();

#[inline(always)]
pub fn process_initialize_global_vault(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Global Vault account (PDA derived from [mint])
    // 1. [signer]   Payer (funds the account creation)
    // 2. []         Mint  (seed)
    // 3. [writable] Vault associated token account
    // 4. []         Token program
    // 5. []         Associated token program
    // 6. []         System program

    let [vault_info, payer_info, mint_info, other_accounts @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let program_id = ephemeral_spl_api::program::id_address();
    if vault_info.owned_by(&program_id) && vault_info.data_len() == GlobalVault::LEN {
        return Ok(());
    }

    let [vault_token_acc_info, token_program_info, associated_token_program_info, system_program_info, ..] =
        other_accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !pinocchio_associated_token_account::check_id(associated_token_program_info.address()) {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (vault, bump) = GlobalVault::find_pda(&mint_info.address());
    if &vault != vault_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump_seed = [bump];
    let seed = GlobalVault::signer_seeds(&mint_info.address(), &bump_seed);
    let signer_seeds = Signer::from(&seed);
    let required_lamports = Rent::get()?.try_minimum_balance(GlobalVault::LEN)?;

    if vault_info.owned_by(&program_id) {
        let vault_data_len = vault_info.data_len();
        if vault_data_len != LEGACY_GLOBAL_VAULT_LEN {
            return Err(ProgramError::InvalidAccountData);
        }

        // Migrate legacy vaults from 32-byte layout (mint only) to 64-byte layout.
        // TODO: Remove this migration path once all deployed vaults are upgraded.
        let legacy_mint = {
            let legacy_data = unsafe { vault_info.borrow_unchecked() };
            unsafe { (*(legacy_data.as_ptr() as *const pinocchio::Address)).clone() }
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
        CreateAccount {
            from: payer_info,
            to: vault_info,
            space: GlobalVault::LEN as u64,
            lamports: required_lamports,
            owner: &program_id,
        }
        .invoke_signed(&[signer_seeds])?;
    }

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
    let vault = unsafe { load_mut_unchecked::<GlobalVault>(vault_info.borrow_unchecked_mut())? };

    // Initialize the vault
    vault.bump = bump;
    vault.mint = mint_info.address().clone();
    vault.token_account = vault_token_acc_info.address().clone();

    Ok(())
}
