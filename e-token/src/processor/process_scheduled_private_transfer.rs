use alloc::vec::Vec;
use core::mem::MaybeUninit;

use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::{invoke_signed_with_bounds, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::processor::initialize_rent_pda::RENT_PDA;
use crate::processor::utils::{is_supported_token_program, read_token_account};

/// Account count on this top-level instruction (mirrors instruction 25's layout).
pub(crate) const SCHEDULED_PT_ACCOUNTS: usize = 19;

/// Fixed prefix of `instruction_data` injected by `schedule_private_transfer`
/// when it baked the Hydra-scheduled payload:
/// `[user_pubkey: 32][stash_pda_bump: 1][shuttle_id: 4]`.
const PREFIX_LEN: usize = 32 + 1 + 4;

///
/// Executes on: BASE only. Top-level, triggered by Hydra.
///
/// Hydra forbids signer metas in scheduled instructions, so every account
/// here arrives non-signer. The processor re-derives the stash PDA from
/// `[b"stash", user, mint]` and `invoke_signed`s into instruction 25 with
/// the stash PDA covering both the `payer` and `owner` signer slots.
///
/// Accounts (match instruction 25's layout verbatim so the self-CPI can
/// forward the slice):
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
///
/// Instruction data (after the entrypoint strips the discriminator):
///
///   00..32 : user pubkey (stash PDA seed)
///   32..33 : stash PDA bump
///   33..37 : shuttle_id (u32 LE)
///   37.... : [len:u8][bytes] optional validator (0 or 32 bytes)
///   ...... : [len:u8][bytes] encrypted destination
///   ...... : [len:u8][bytes] encrypted data suffix
///
#[inline(never)]
pub fn process_scheduled_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [stash_payer_info, rent_pda_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, stash_owner_info, owner_program_info, buffer_info, delegation_record_info, delegation_metadata_info, delegation_program_info, associated_token_program_info, system_program_info, mint_info, token_program_info, global_vault_info, stash_ata_info, vault_token_info, queue_info] =
        require_n_accounts!(accounts, 19);

    require!(
        instruction_data.len() >= PREFIX_LEN,
        ProgramError::InvalidInstructionData
    );

    // -------- parse prefix --------
    let user = Address::new_from_array(
        instruction_data[0..32]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let stash_bump = instruction_data[32];
    let shuttle_id_bytes = &instruction_data[33..37];
    let tail = &instruction_data[37..];
    let mut tail_cursor = 0;
    let validator_bytes = read_vardata(tail, &mut tail_cursor)?;
    read_vardata(tail, &mut tail_cursor)?;
    read_vardata(tail, &mut tail_cursor)?;
    require!(
        tail_cursor == tail.len(),
        ProgramError::InvalidInstructionData
    );
    require!(
        validator_bytes.is_empty() || validator_bytes.len() == 32,
        ProgramError::InvalidInstructionData
    );

    // -------- validate stash PDA derivation --------
    let derived_stash = StashPda::derive_pda(&user, mint_info.address(), stash_bump)?;
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

    // -------- sweep: amount = current stash ATA token balance --------
    let effective_amount = read_token_account(stash_ata_info)?.amount();
    require!(effective_amount != 0, ProgramError::InvalidAccountData);

    // -------- build ix 25 instruction data --------
    //   [25][shuttle_id:4][amount:8][vardata tail]
    let mut ix_data: Vec<u8> = Vec::with_capacity(1 + 4 + 8 + tail.len());
    ix_data.push(
        ephemeral_spl_api::instruction::DEPOSIT_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE_AND_PRIVATE_TRANSFER,
    );
    ix_data.extend_from_slice(shuttle_id_bytes);
    ix_data.extend_from_slice(&effective_amount.to_le_bytes());
    ix_data.extend_from_slice(tail);

    // -------- build ix 25 account metas (19) --------
    let mut metas = [const { MaybeUninit::<InstructionAccount>::uninit() }; SCHEDULED_PT_ACCOUNTS];
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
                SCHEDULED_PT_ACCOUNTS,
            )
        },
        data: &ix_data,
    };

    // Single PDA signer authorizes both slot 0 (payer) and slot 5 (owner)
    // because they reference the same stash PDA.
    let stash_bump_seed = [stash_bump];
    let stash_signer_seeds = StashPda::signer_seeds(&user, mint_info.address(), &stash_bump_seed);
    let stash_signer = Signer::from(&stash_signer_seeds);

    let account_refs: [&AccountView; SCHEDULED_PT_ACCOUNTS] = [
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

    invoke_signed_with_bounds::<SCHEDULED_PT_ACCOUNTS>(&instruction, &account_refs, &[stash_signer])
}

#[inline(always)]
fn read_vardata<'a>(data: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], ProgramError> {
    require!(*cursor < data.len(), ProgramError::InvalidInstructionData);
    let len = data[*cursor] as usize;
    let start = *cursor + 1;
    let end = start
        .checked_add(len)
        .ok_or(ProgramError::InvalidInstructionData)?;
    require!(end <= data.len(), ProgramError::InvalidInstructionData);
    *cursor = end;
    Ok(&data[start..end])
}
