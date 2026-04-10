use core::marker::PhantomData;

use {
    ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer,
    ephemeral_spl_api::state::{
        global_vault::GlobalVault, transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA,
    },
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

use crate::{
    assert_owner, assert_signer,
    processor::{
        initialize_rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
        internal::token_vault::validate_vault_for_mint,
        utils::read_mint_decimals,
    },
};
use pinocchio::{
    address::address_eq,
    cpi::{Seed, Signer},
};
use pinocchio_system::ID as SYSTEM_PROGRAM_ID;

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
    let args = ExecuteQueuedTransferArgs::try_from_bytes(instruction_data)?;
    let [vault_info, mint_info, vault_token_acc_info, destination_owner_info, destination_token_acc_info, rent_pda_info, token_program_info, associated_token_program_info, system_program_info, source_program, escrow_authority, escrow_signer] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Note that accounts [source_program, escrow_authority, escrow_signer] are appended by DLP's
    // CallHandlerV2 instruction.
    if !address_eq(source_program.address(), &crate::ID) {
        return Err(ProgramError::IncorrectAuthority);
    }

    assert_signer!(escrow_signer);

    let expected_escrow =
        ephemeral_balance_pda_from_payer(escrow_authority.address(), args.escrow_index());
    if !address_eq(&expected_escrow, escrow_signer.address()) {
        return Err(ProgramError::InvalidSeeds);
    }

    if args.should_create_destination_ata_idempotent() {
        assert_owner!(rent_pda_info, &SYSTEM_PROGRAM_ID);
        if !address_eq(&RENT_PDA, rent_pda_info.address()) {
            return Err(ProgramError::InvalidSeeds);
        }
        if rent_pda_info.data_len() != 0 {
            return Err(ProgramError::InvalidAccountData);
        }
        if !address_eq(
            associated_token_program_info.address(),
            &pinocchio_associated_token_account::ID,
        ) || !address_eq(system_program_info.address(), &SYSTEM_PROGRAM_ID)
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

///
/// DataLayout:
///
///     00..01 : escrow_index (u8)
///     01..09 : amount (u64)
///     09..10 : flags (u8)
///
/// ValidLength:
///
///     10
///
pub struct ExecuteQueuedTransferArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl ExecuteQueuedTransferArgs<'_> {
    const LEN: usize = 10;
    const LEN_WITH_CLIENT_REF_ID: usize = 18;

    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<ExecuteQueuedTransferArgs<'_>, ProgramError> {
        if bytes.len() != Self::LEN && bytes.len() != Self::LEN_WITH_CLIENT_REF_ID {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(ExecuteQueuedTransferArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
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

    #[inline]
    pub fn client_ref_id(&self) -> Option<u64> {
        if self.len != Self::LEN_WITH_CLIENT_REF_ID {
            return None;
        }

        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(10), buf.as_mut_ptr(), 8);
        }
        Some(u64::from_le_bytes(buf))
    }
}
