use core::marker::PhantomData;
use ephemeral_spl_api::error::EphemeralSplError;
use pinocchio::cpi::Signer;
use {
    ephemeral_spl_api::state::{
        ephemeral_ata::load_ephemeral_ata_compat_mut, global_vault::GlobalVault, load_unchecked,
    },
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
    pinocchio_token_2022::state::Mint,
};

#[inline(always)]
pub fn process_withdraw_spl_tokens(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Owner (payer, authority to withdraw)
    // 1. [writable] Ephemeral ATA data account (PDA [owner, mint])
    // 2. []         Global Vault data account (PDA [mint])
    // 3. []         Mint account (readonly)
    // 4. [writable] Vault source token account (SPL Token)
    // 5. [writable] User destination token account (SPL Token)
    // 6. []         Token program

    let args = WithdrawArgs::try_from_bytes(instruction_data)?;

    let [owner, ephemeral_ata_info, vault_info, mint_info, vault_source_token_acc, user_dest_token_acc, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    withdraw_ephemeral_ata_tokens(
        owner,
        true,
        ephemeral_ata_info,
        vault_info,
        mint_info,
        vault_source_token_acc,
        user_dest_token_acc,
        token_program_info,
        args.amount(),
    )
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn withdraw_ephemeral_ata_tokens(
    owner: &AccountView,
    require_owner_signature: bool,
    ephemeral_ata_info: &AccountView,
    vault_info: &AccountView,
    mint_info: &AccountView,
    vault_source_token_acc: &AccountView,
    user_dest_token_acc: &AccountView,
    token_program_info: &AccountView,
    amount: u64,
) -> ProgramResult {
    if require_owner_signature && !owner.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Validate EphemeralAta account (writable)
    unsafe {
        if ephemeral_ata_info.owner().ne(&ephemeral_spl_api::ID) {
            return Err(ProgramError::IllegalOwner);
        }
    }
    let mut ephemeral_ata =
        load_ephemeral_ata_compat_mut(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;

    // Validate vault ownership before reading raw data.
    unsafe {
        if vault_info.owner().ne(&ephemeral_spl_api::ID) {
            return Err(ProgramError::IllegalOwner);
        }
    }

    // Validate Vault data account
    let vault = unsafe { load_unchecked::<GlobalVault>(vault_info.borrow_unchecked())? };

    // Check eata consistency
    if ephemeral_ata.mint() != mint_info.address()
        || vault.mint != *mint_info.address()
        || ephemeral_ata.owner() != owner.address()
        || vault.token_account != *vault_source_token_acc.address()
    {
        return Err(EphemeralSplError::EphemeralAtaMismatch.into());
    }

    // Parse the base mint layout shared by both legacy SPL Token and Token-2022.
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

    // Perform transfer from vault token account to user destination, signed by vault PDA
    let bump = [vault.bump];
    let seeds = GlobalVault::signer_seeds(mint_info.address(), &bump);
    let signer = Signer::from(&seeds);

    pinocchio_token_2022::instructions::TransferChecked {
        mint: mint_info,
        from: vault_source_token_acc,
        to: user_dest_token_acc,
        authority: vault_info, // PDA authority over the vault token account
        token_program: token_program_info.address(),
        amount,
        decimals,
    }
    .invoke_signed(&[signer])?;

    // Safely decrease the amount in the EphemeralAta
    let updated_amount = ephemeral_ata
        .amount()
        .checked_sub(amount)
        .ok_or(ProgramError::InvalidArgument)?;
    ephemeral_ata.set_amount(updated_amount);

    Ok(())
}

/// Instruction data for the `WithdrawSplTokens` instruction.
pub struct WithdrawArgs<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl WithdrawArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<WithdrawArgs<'_>, ProgramError> {
        if bytes.len() != 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(WithdrawArgs {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        // read LE u64 from bytes[0..8]
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }
}
