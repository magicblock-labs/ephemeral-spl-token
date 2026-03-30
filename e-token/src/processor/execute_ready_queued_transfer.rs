use core::marker::PhantomData;

use {
    ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer,
    ephemeral_spl_api::state::{
        global_vault::GlobalVault, transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
    },
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

use ephemeral_spl_api::state::load_initialized;
use pinocchio::cpi::{Seed, Signer};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;
use ephemeral_spl_api::program::id_address;
use crate::{
    assert_owner,
    processor::{
        rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
        utils::read_mint_decimals,
    },
};

#[inline(always)]
pub fn process_execute_ready_queued_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = ExecuteQueuedTransferArgs::try_from_bytes(instruction_data)?;

    // Expected accounts:
    // 0. []         Global vault PDA
    // 1. []         Mint account
    // 2. [writable] Vault token account
    // 3. []         Destination owner
    // 4. [writable] Destination token account
    // 5. [writable] Global rent PDA
    // 6. []         Token program
    // 7. []         Associated token program
    // 8. []         System program
    // 9. []         Source program (must equal this program)
    // 10. []        Escrow authority
    // 11. [signer]  Escrow signer PDA
    let [vault_info, mint_info, vault_token_acc_info, destination_owner_info, destination_token_acc_info, rent_pda_info, token_program_info, associated_token_program_info, system_program_info, source_program, escrow_authority, escrow_signer] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if source_program.address() != &id_address() {
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

    if args.should_create_destination_ata_idempotent() {
        assert_owner!(rent_pda_info, &SYSTEM_PROGRAM_ID);
        if &RENT_PDA != rent_pda_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }
        if rent_pda_info.data_len() != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if associated_token_program_info.address() != &pinocchio_associated_token_account::ID
            || system_program_info.address() != &SYSTEM_PROGRAM_ID
        {
            return Err(ProgramError::InvalidAccountData);
        }

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
    assert_owner!(vault_info, &ephemeral_spl_api::program::id_address());

    let vault = load_initialized::<GlobalVault>(unsafe { vault_info.borrow_unchecked() })?;
    let (derived_vault, bump) = ephemeral_spl_api::Address::find_program_address(
        &[mint_info.address().as_ref()],
        &ephemeral_spl_api::program::id_address(),
    );
    if derived_vault != *vault_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if vault.mint != *mint_info.address() || vault.token_account != *vault_token_acc_info.address()
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
    const LEN: usize = 10;

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

    #[inline]
    pub fn flags(&self) -> u8 {
        unsafe { *self.raw.add(9) }
    }

    #[inline]
    pub fn should_create_destination_ata_idempotent(&self) -> bool {
        self.flags() & QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA != 0
    }
}
