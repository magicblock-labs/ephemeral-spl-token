use alloc::borrow::ToOwned;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::MaybeUninit;

use data_layout::variable_offset_layout;
use ephemeral_spl_api::instruction::ESplInstruction;

use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::{invoke_signed_with_bounds, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::sysvars::{clock::Clock, Sysvar};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::Transfer;
use pinocchio_token_2022::instructions::{CloseAccount, TransferChecked};

use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::internal::derive_hydra_seed;
use crate::processor::utils::{
    get_associated_token_address, is_supported_token_program, read_mint_decimals,
    validate_token_account,
};
use crate::DepositAndDelegateShuttleWithPrivateTransferArgs;

/// Number of metas forwarded to instruction 25.
pub(crate) const SCHEDULED_PT_INNER_ACCOUNTS: usize = 19;

/// Total accounts on this top-level instruction (ix 25 layout + user ATA + Hydra crank).
pub(crate) const SCHEDULED_PT_ACCOUNTS: usize = SCHEDULED_PT_INNER_ACCOUNTS + 2;

// Five minutes at an estimated 400 ms/slot.
const REFUND_TIMEOUT_SLOTS: u64 = 750;

///
/// Executes on: BASE only. Top-level, triggered by Hydra.
///
/// Hydra forbids signer metas in scheduled instructions, so every account
/// here arrives non-signer. The processor re-derives the stash PDA from
/// `[b"stash", user, mint]` and `invoke_signed`s into instruction 25 with
/// the stash PDA covering both the `payer` and `owner` signer slots.
///
/// Accounts: slots 0..18 mirror instruction 25's layout verbatim so the
/// self-CPI can forward that prefix unchanged. Slot 20 is the Hydra crank
/// PDA, the provenance witness — `crank.authority == RENT_PDA` and
/// `crank.authority_signer == 1` proves ix 29 created the schedule.
///
///  0: [writable]          - PDA     : Stash PDA (payer in ix 25).
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Shuttle metadata account.
///  3: [writable]          - PDA     : Shuttle EATA account.
///  4: [writable]          - SPL     : Shuttle wallet ATA account.
///  5: []                  - PDA     : Stash PDA (owner in ix 25; same key as 0).
///  6: []                  - Program : Owner program (this program).
///  7: [writable]          - PDA     : Buffer account.
///  8: [writable]          - PDA     : Delegation record account.
///  9: [writable]          - PDA     : Delegation metadata account.
/// 10: []                  - Program : Delegation program.
/// 11: []                  - SPL     : Associated token program.
/// 12: []                  - Builtin : System program.
/// 13: []                  - SPL     : Mint account.
/// 14: []                  - SPL     : Token program.
/// 15: []                  - PDA     : Global vault account.
/// 16: [writable]          - SPL     : Stash ATA (owner_source_token in ix 25).
/// 17: [writable]          - SPL     : Vault token account.
/// 18: [writable]          - PDA     : Transfer queue account.
/// 19: [writable]          - SPL     : User ATA for timeout refund.
/// 20: [writable]          - PDA     : Hydra crank PDA (provenance witness;
///                                     writable in the meta only because
///                                     Trigger marks it writable at slot 0,
///                                     and Solana's sysvar serializes the
///                                     tx-level writable union).
///
/// Instruction Data: ExecuteScheduledPrivateTransferArgs
///
#[inline(never)]
pub fn process_execute_scheduled_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        stash_payer_info, // force multi-line
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        stash_owner_info,
        owner_program_info,
        buffer_info,
        delegation_record_info,
        delegation_metadata_info,
        delegation_program_info,
        associated_token_program_info,
        system_program_info,
        mint_info,
        token_program_info,
        global_vault_info,
        stash_ata_info,
        vault_token_info,
        queue_info,
        user_ata_info,
        hydra_crank_info
    ] = require_n_accounts!(accounts, 21);

    let args = ExecuteScheduledPrivateTransferArgs::decode(instruction_data)?;

    // -------- validate stash PDA derivation --------
    let derived_stash =
        StashPda::derive_pda(args.user_address(), mint_info.address(), args.stash_bump())?;
    require_eq_keys!(
        &derived_stash,
        stash_payer_info.address(),
        ProgramError::InvalidSeeds
    );
    require_eq_keys!(
        stash_owner_info.address(),
        stash_payer_info.address(),
        ProgramError::InvalidSeeds
    );
    require_eq_keys!(
        rent_pda_info.address(),
        &RENT_PDA,
        ProgramError::InvalidSeeds
    );
    require!(
        is_supported_token_program(token_program_info.address()),
        ProgramError::IncorrectProgramId
    );

    require!(
        hydra_crank_info.owned_by(&Address::new_from_array(hydra_api::ID.to_bytes())),
        ProgramError::InvalidAccountOwner
    );
    let hydra_seed = derive_hydra_seed(stash_payer_info.address(), args.shuttle_id());
    let (expected_crank, _) = hydra_api::state::find_crank_pda(&hydra_seed);
    require_eq_keys!(
        &expected_crank,
        hydra_crank_info.address(),
        ProgramError::InvalidSeeds
    );
    let crank_data = hydra_crank_info.try_borrow()?;
    let crank = unsafe { hydra_api::state::load_crank(&crank_data)? };
    let crank_authority = Address::new_from_array(crank.authority);
    require_eq_keys!(&crank_authority, &RENT_PDA, ProgramError::InvalidSeeds);
    require!(
        crank.authority_signer == 1,
        ProgramError::InvalidAccountData
    );
    let scheduled_slot = if crank.executed() == 0 {
        crank.next_exec_slot()
    } else {
        crank
            .next_exec_slot()
            .saturating_sub(crank.interval_slots())
    };
    drop(crank_data);

    // -------- sweep: amount = current stash ATA token balance --------
    let effective_amount = read_stash_ata_amount(
        stash_payer_info,
        stash_ata_info,
        mint_info,
        token_program_info,
    )?;
    let stash_bump_seed = [args.stash_bump()];
    let stash_signer_seeds =
        StashPda::signer_seeds(args.user_address(), mint_info.address(), &stash_bump_seed);
    let stash_signer = Signer::from(&stash_signer_seeds);

    if effective_amount == 0 {
        close_empty_stash_accounts(
            stash_payer_info,
            rent_pda_info,
            stash_ata_info,
            mint_info,
            token_program_info,
            &stash_signer,
        )?;
        return Ok(());
    }

    if refund_timeout_elapsed(scheduled_slot)? {
        let expected_user_ata = get_associated_token_address(
            args.user_address(),
            mint_info.address(),
            token_program_info.address(),
        );
        require_eq_keys!(
            &expected_user_ata,
            user_ata_info.address(),
            ProgramError::InvalidSeeds
        );
        validate_token_account(
            user_ata_info,
            mint_info.address(),
            Some(args.user_address()),
            Some(token_program_info.address()),
        )?;

        let decimals = read_mint_decimals(mint_info, token_program_info)?;
        TransferChecked {
            mint: mint_info,
            from: stash_ata_info,
            to: user_ata_info,
            authority: stash_payer_info,
            token_program: token_program_info.address(),
            amount: effective_amount,
            decimals,
        }
        .invoke_signed(core::slice::from_ref(&stash_signer))?;
        close_empty_stash_accounts(
            stash_payer_info,
            rent_pda_info,
            stash_ata_info,
            mint_info,
            token_program_info,
            &stash_signer,
        )?;
        return Ok(());
    }

    // -------- build ix 25 instruction data --------
    let ix_data = ESplInstruction::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer
        .with_data(
            &DepositAndDelegateShuttleWithPrivateTransferArgs {
                shuttle_id: args.shuttle_id(),
                amount: effective_amount,
                exact_out: false,
                validator: Some(args.validator().to_owned()),
                encrypted_destination: args.encrypted_destination().to_owned(),
                encrypted_data_suffix: args.encrypted_data_suffix().to_owned(),
            }
            .encode()?,
        );

    // -------- build ix 25 account metas (19) --------
    let mut metas =
        [const { MaybeUninit::<InstructionAccount>::uninit() }; SCHEDULED_PT_INNER_ACCOUNTS];
    unsafe {
        metas
            .get_unchecked_mut(0)
            .write(InstructionAccount::writable_signer(
                stash_payer_info.address(),
            ));
        metas
            .get_unchecked_mut(1)
            .write(InstructionAccount::writable(rent_pda_info.address()));
        metas
            .get_unchecked_mut(2)
            .write(InstructionAccount::writable(shuttle_info.address()));
        metas
            .get_unchecked_mut(3)
            .write(InstructionAccount::writable(shuttle_eata_info.address()));
        metas
            .get_unchecked_mut(4)
            .write(InstructionAccount::writable(
                shuttle_wallet_ata_info.address(),
            ));
        metas
            .get_unchecked_mut(5)
            .write(InstructionAccount::readonly_signer(
                stash_owner_info.address(),
            ));
        metas
            .get_unchecked_mut(6)
            .write(InstructionAccount::readonly(owner_program_info.address()));
        metas
            .get_unchecked_mut(7)
            .write(InstructionAccount::writable(buffer_info.address()));
        metas
            .get_unchecked_mut(8)
            .write(InstructionAccount::writable(
                delegation_record_info.address(),
            ));
        metas
            .get_unchecked_mut(9)
            .write(InstructionAccount::writable(
                delegation_metadata_info.address(),
            ));
        metas
            .get_unchecked_mut(10)
            .write(InstructionAccount::readonly(
                delegation_program_info.address(),
            ));
        metas
            .get_unchecked_mut(11)
            .write(InstructionAccount::readonly(
                associated_token_program_info.address(),
            ));
        metas
            .get_unchecked_mut(12)
            .write(InstructionAccount::readonly(system_program_info.address()));
        metas
            .get_unchecked_mut(13)
            .write(InstructionAccount::readonly(mint_info.address()));
        metas
            .get_unchecked_mut(14)
            .write(InstructionAccount::readonly(token_program_info.address()));
        metas
            .get_unchecked_mut(15)
            .write(InstructionAccount::readonly(global_vault_info.address()));
        metas
            .get_unchecked_mut(16)
            .write(InstructionAccount::writable(stash_ata_info.address()));
        metas
            .get_unchecked_mut(17)
            .write(InstructionAccount::writable(vault_token_info.address()));
        metas
            .get_unchecked_mut(18)
            .write(InstructionAccount::writable(queue_info.address()));
    }

    let instruction = InstructionView {
        program_id: &crate::ID,
        accounts: unsafe {
            core::slice::from_raw_parts(
                metas.as_ptr() as *const InstructionAccount,
                SCHEDULED_PT_INNER_ACCOUNTS,
            )
        },
        data: &ix_data,
    };

    // Single PDA signer authorizes both slot 0 (payer) and slot 5 (owner)
    // because they reference the same stash PDA.
    let account_refs: [&AccountView; SCHEDULED_PT_INNER_ACCOUNTS] = [
        stash_payer_info,
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        stash_owner_info,
        owner_program_info,
        buffer_info,
        delegation_record_info,
        delegation_metadata_info,
        delegation_program_info,
        associated_token_program_info,
        system_program_info,
        mint_info,
        token_program_info,
        global_vault_info,
        stash_ata_info,
        vault_token_info,
        queue_info,
    ];

    invoke_signed_with_bounds::<SCHEDULED_PT_INNER_ACCOUNTS>(
        &instruction,
        &account_refs,
        &[stash_signer.clone()],
    )?;

    close_empty_stash_accounts(
        stash_payer_info,
        rent_pda_info,
        stash_ata_info,
        mint_info,
        token_program_info,
        &stash_signer,
    )
}

#[variable_offset_layout(buffer_offset = 1)]
pub struct ExecuteScheduledPrivateTransferArgs {
    pub user: [u8; 32],
    pub stash_bump: u8,
    pub shuttle_id: u32,
    pub validator: [u8; 32],
    pub encrypted_destination: [u8; 80],
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}

impl ExecuteScheduledPrivateTransferArgsView<'_> {
    fn user_address(&self) -> &Address {
        unsafe { &*(self.user().as_ptr() as *const Address) }
    }
}

#[inline(always)]
fn refund_timeout_elapsed(scheduled_slot: u64) -> Result<bool, ProgramError> {
    let refund_slot = scheduled_slot
        .checked_add(REFUND_TIMEOUT_SLOTS)
        .ok_or(ProgramError::InvalidInstructionData)?;
    Ok(Clock::get()?.slot > refund_slot)
}

#[inline(always)]
fn read_stash_ata_amount(
    stash_pda_info: &AccountView,
    stash_ata_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
) -> Result<u64, ProgramError> {
    let expected_stash_ata = get_associated_token_address(
        stash_pda_info.address(),
        mint_info.address(),
        token_program_info.address(),
    );
    require_eq_keys!(
        &expected_stash_ata,
        stash_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    if stash_ata_info.lamports() == 0 {
        return Ok(0);
    }

    Ok(validate_token_account(
        stash_ata_info,
        mint_info.address(),
        Some(stash_pda_info.address()),
        Some(token_program_info.address()),
    )?
    .amount())
}

#[inline(always)]
fn close_empty_stash_accounts<'a, 'b>(
    stash_pda_info: &AccountView,
    rent_pda_info: &AccountView,
    stash_ata_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    stash_signer: &Signer<'a, 'b>,
) -> ProgramResult {
    let stash_ata_amount = read_stash_ata_amount(
        stash_pda_info,
        stash_ata_info,
        mint_info,
        token_program_info,
    )?;
    require!(stash_ata_amount == 0, ProgramError::InvalidArgument);

    if stash_ata_info.lamports() > 0 {
        CloseAccount {
            account: stash_ata_info,
            destination: rent_pda_info,
            authority: stash_pda_info,
            token_program: token_program_info.address(),
        }
        .invoke_signed(core::slice::from_ref(stash_signer))?;
    }

    let stash_lamports = stash_pda_info.lamports();
    if stash_lamports > 0 {
        Transfer {
            from: stash_pda_info,
            to: rent_pda_info,
            lamports: stash_lamports,
        }
        .invoke_signed(core::slice::from_ref(stash_signer))?;
    }

    Ok(())
}
