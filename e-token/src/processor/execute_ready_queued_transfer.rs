use core::marker::PhantomData;

use {
    ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer,
    ephemeral_spl_api::state::{global_vault::GlobalVault, load_unchecked},
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
    pinocchio_token_2022::state::Mint,
};

use pinocchio::cpi::{Seed, Signer};

#[inline(always)]
pub fn process_execute_ready_queued_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = ExecuteQueuedTransferArgs::try_from_bytes(instruction_data)?;

    // Expected accounts:
    // 0. []         Queue crank payer
    // 1. []         Global vault PDA
    // 2. []         Mint account
    // 3. [writable] Vault token account
    // 4. [writable] Destination token account
    // 5. []         Token program
    //
    // Expected trailing accounts from tail:
    // - second to last: escrow authority
    // - last:          escrow signer PDA
    if accounts.len() < 8 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let queue_payer_info = &accounts[0];
    let vault_info = &accounts[1];
    let mint_info = &accounts[2];
    let vault_token_acc_info = &accounts[3];
    let destination_token_acc_info = &accounts[4];
    let token_program_info = &accounts[5];
    let escrow_authority = &accounts[accounts.len() - 2];
    let escrow_signer = &accounts[accounts.len() - 1];

    if escrow_authority.address() != queue_payer_info.address() {
        return Err(ProgramError::IncorrectAuthority);
    }
    if !escrow_signer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let expected_escrow =
        ephemeral_balance_pda_from_payer(escrow_authority.address(), args.escrow_index());
    if expected_escrow != *escrow_signer.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let vault_bump = validate_vault_for_mint(vault_info, mint_info, vault_token_acc_info)?;
    let decimals = {
        let mint_data = unsafe { mint_info.borrow_unchecked() };
        if mint_data.len() < Mint::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
        if !mint.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        mint.decimals()
    };

    let vault_bump = [vault_bump];
    let signer_seeds = [
        Seed::from(mint_info.address().as_ref()),
        Seed::from(&vault_bump),
    ];
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

    Ok(())
}

#[inline(always)]
pub(crate) fn validate_vault_for_mint(
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_token_acc_info: &AccountView,
) -> Result<u8, ProgramError> {
    unsafe {
        if vault_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let vault = unsafe { load_unchecked::<GlobalVault>(vault_info.borrow_unchecked())? };
    let (derived_vault, bump) = ephemeral_spl_api::Address::find_program_address(
        &[mint_info.address().as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_vault != *vault_info.address()
        || vault.mint != *mint_info.address()
        || vault.token_account != *vault_token_acc_info.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(bump)
}

pub struct ExecuteQueuedTransferArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl ExecuteQueuedTransferArgs<'_> {
    const LEN: usize = 9;

    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<ExecuteQueuedTransferArgs<'_>, ProgramError> {
        if bytes.len() != Self::LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(ExecuteQueuedTransferArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(1), buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }

    #[inline]
    pub fn escrow_index(&self) -> u8 {
        unsafe { *self.raw }
    }
}
