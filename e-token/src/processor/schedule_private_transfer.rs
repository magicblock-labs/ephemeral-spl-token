use alloc::vec::Vec;

use ephemeral_rollups_pinocchio::consts::{
    BUFFER, DELEGATION_METADATA, DELEGATION_PROGRAM_ID, DELEGATION_RECORD,
};
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};

use hydra_api::consts::CRANKER_REWARD;
use hydra_api::instruction::{self as hydra_ix, CreateArgs, SchedMeta};

use pinocchio::cpi::{invoke_signed_with_bounds, Seed, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::sysvars::{clock::Clock, Sysvar};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::Transfer;
use solana_pubkey::Pubkey;

use crate::processor::initialize_rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED};
use crate::processor::process_scheduled_private_transfer::SCHEDULED_PT_ACCOUNTS;
use crate::processor::utils::is_supported_token_program;

const FIXED_PREFIX_LEN: usize = 4 + 1 + 32 + 10;

const SETUP_LAMPORTS: u64 = ephemeral_spl_api::consts::SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS
    + ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS;

///
/// Executes on: BASE only. User-signed.
///
/// Appended to a swap transaction to schedule a private transfer
/// (instruction 25 via Hydra) over whatever balance ends up in the stash
/// ATA when the crank fires. Keeps the outer ix small: every account that
/// would only be read for its pubkey is derived on-chain using the bumps
/// supplied in the instruction data; hard-coded program IDs stand in for
/// DLP, system, ATA, and Hydra programs.
///
/// Accounts (7):
///
///  0: [signer]            - Keypair : User who owns the stash PDA.
///  1: [writable]          - PDA     : Stash PDA. Seeds `[b"stash", user, mint]`.
///  2: [writable]          - PDA     : Rent PDA. Funds the Hydra crank.
///  3: [writable]          - PDA     : Hydra crank PDA. Derived via `derive_hydra_seed`,
///                                     which mixes the stash PDA and `shuttle_id`, so the
///                                     crank PDA is unique per stash+shuttle schedule.
///  4: []                  - Program : Hydra program.
///  5: []                  - Builtin : System program.
///  6: []                  - SPL     : Token program (Token / Token-2022) used as an ATA seed.
///
/// Instruction data (47 B fixed prefix + 3 vardata):
///
///   00..04  shuttle_id (u32 LE)
///   04      stash_pda_bump
///   05..37  mint (32 B)
///   37..47  10 bumps: shuttle, shuttle_eata, shuttle_wallet_ata, buffer,
///           delegation_record, delegation_metadata, global_vault,
///           vault_token, stash_ata, queue.
///   47..    [len:u8] validator pubkey (0 or 32 bytes)
///   ....    [len:u8] encrypted destination (pass-through to ix 25)
///   ....    [len:u8] encrypted data suffix (pass-through to ix 25)
///
#[inline(never)]
pub fn process_schedule_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        user_info,
        stash_pda_info,
        rent_pda_info,
        hydra_crank_pda_info,
        _hydra_program_info, // required in tx account list for the Hydra CPI
        system_program_info,
        token_program_info,
    ] = require_n_accounts!(accounts, 7);

    require!(
        instruction_data.len() >= FIXED_PREFIX_LEN,
        ProgramError::InvalidInstructionData
    );

    let shuttle_id_bytes: [u8; 4] = instruction_data[0..4]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let stash_bump = instruction_data[4];
    let mint = Address::new_from_array(
        instruction_data[5..37]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let shuttle_bump = instruction_data[37];
    let shuttle_eata_bump = instruction_data[38];
    let shuttle_wallet_ata_bump = instruction_data[39];
    let buffer_bump = instruction_data[40];
    let delegation_record_bump = instruction_data[41];
    let delegation_metadata_bump = instruction_data[42];
    let global_vault_bump = instruction_data[43];
    let vault_token_bump = instruction_data[44];
    let stash_ata_bump = instruction_data[45];
    let queue_bump = instruction_data[46];

    let mut cursor = FIXED_PREFIX_LEN;
    let validator_bytes = read_vardata(instruction_data, &mut cursor)?;
    read_vardata(instruction_data, &mut cursor)?;
    read_vardata(instruction_data, &mut cursor)?;
    require!(
        cursor == instruction_data.len(),
        ProgramError::InvalidInstructionData
    );

    require!(
        validator_bytes.is_empty() || validator_bytes.len() == 32,
        ProgramError::InvalidInstructionData
    );
    let validator = if validator_bytes.is_empty() {
        Address::default()
    } else {
        Address::new_from_array(
            validator_bytes
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        )
    };

    let derived_stash = StashPda::derive_pda(user_info.address(), &mint, stash_bump)?;
    require_eq_keys!(
        &derived_stash,
        stash_pda_info.address(),
        ProgramError::InvalidSeeds
    );
    require_eq_keys!(
        rent_pda_info.address(),
        &RENT_PDA,
        ProgramError::InvalidSeeds
    );

    let token_program_id = *token_program_info.address();
    require!(
        is_supported_token_program(&token_program_id),
        ProgramError::IncorrectProgramId
    );

    Transfer {
        from: user_info,
        to: stash_pda_info,
        lamports: SETUP_LAMPORTS,
    }
    .invoke()?;

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seeds);

    let shuttle = ShuttleMetadata::derive_pda(
        stash_pda_info.address(),
        &mint,
        u32::from_le_bytes(shuttle_id_bytes),
        shuttle_bump,
    )?;
    let shuttle_eata = EphemeralAta::derive_pda(&shuttle, &mint, shuttle_eata_bump)?;
    let shuttle_wallet_ata =
        derive_ata(&shuttle, &token_program_id, &mint, shuttle_wallet_ata_bump)?;
    let buffer = Address::create_program_address(
        &[BUFFER, shuttle_eata.as_ref(), &[buffer_bump]],
        &crate::ID,
    )?;
    let delegation_record = Address::create_program_address(
        &[
            DELEGATION_RECORD,
            shuttle_eata.as_ref(),
            &[delegation_record_bump],
        ],
        &DELEGATION_PROGRAM_ID,
    )?;
    let delegation_metadata = Address::create_program_address(
        &[
            DELEGATION_METADATA,
            shuttle_eata.as_ref(),
            &[delegation_metadata_bump],
        ],
        &DELEGATION_PROGRAM_ID,
    )?;
    let global_vault = GlobalVault::derive_pda(&mint, global_vault_bump)?;
    let vault_token = derive_ata(&global_vault, &token_program_id, &mint, vault_token_bump)?;
    let stash_ata = derive_ata(
        stash_pda_info.address(),
        &token_program_id,
        &mint,
        stash_ata_bump,
    )?;
    let queue = TransferQueue::derive_pda(&mint, &validator, queue_bump)?;

    // Slots 0..19 mirror ix 25's layout. Slot 5 aliases slot 0 (stash PDA);
    // the flag must match Solana's tx-level writable union, so both are
    // writable. Slot 19 is the Hydra crank PDA (provenance witness for
    // ix 29).
    let sched_metas: [(&Address, bool); SCHEDULED_PT_ACCOUNTS] = [
        (stash_pda_info.address(), true),
        (rent_pda_info.address(), true),
        (&shuttle, true),
        (&shuttle_eata, true),
        (&shuttle_wallet_ata, true),
        (stash_pda_info.address(), true),
        (&crate::ID, false),
        (&buffer, true),
        (&delegation_record, true),
        (&delegation_metadata, true),
        (&DELEGATION_PROGRAM_ID, false),
        (&pinocchio_associated_token_account::ID, false),
        (system_program_info.address(), false),
        (&mint, false),
        (&token_program_id, false),
        (&global_vault, false),
        (&stash_ata, true),
        (&vault_token, true),
        (&queue, true),
        (hydra_crank_pda_info.address(), false),
    ];

    let vardata_tail = &instruction_data[FIXED_PREFIX_LEN..cursor];
    let scheduled_data_len = 1 + 32 + 1 + 4 + vardata_tail.len();
    let mut scheduled_data: Vec<u8> = Vec::with_capacity(scheduled_data_len);
    scheduled_data.push(ephemeral_spl_api::instruction::PROCESS_SCHEDULED_PRIVATE_TRANSFER);
    scheduled_data.extend_from_slice(user_info.address().as_ref());
    scheduled_data.push(stash_bump);
    scheduled_data.extend_from_slice(&shuttle_id_bytes);
    scheduled_data.extend_from_slice(vardata_tail);

    let hydra_program_id = Address::new_from_array(hydra_api::ID.to_bytes());
    let hydra_seed = derive_hydra_seed(stash_pda_info.address(), &shuttle_id_bytes);
    let (derived_crank_pda, _) = hydra_api::state::find_crank_pda(&hydra_seed);
    require_eq_keys!(
        &derived_crank_pda,
        hydra_crank_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    let sched_metas_vec: Vec<SchedMeta> = sched_metas
        .iter()
        .map(|(key, writable)| {
            let pk = Pubkey::new_from_array(*key.as_array());
            if *writable {
                SchedMeta::writable(pk)
            } else {
                SchedMeta::readonly(pk)
            }
        })
        .collect();
    let create_ix = hydra_ix::create(
        Pubkey::new_from_array(*rent_pda_info.address().as_array()),
        Pubkey::new_from_array(*hydra_crank_pda_info.address().as_array()),
        &CreateArgs {
            seed: hydra_seed,
            authority: *rent_pda_info.address().as_array(),
            start_slot: Clock::get()?.slot,
            interval_slots: 1,
            remaining: 1,
            priority_tip: 0,
            cu_limit: 0,
            scheduled_program_id: Pubkey::new_from_array(*crate::ID.as_array()),
            scheduled_metas: &sched_metas_vec,
            scheduled_data: &scheduled_data,
        },
    );

    let hydra_metas = [
        InstructionAccount::writable_signer(rent_pda_info.address()),
        InstructionAccount::writable(hydra_crank_pda_info.address()),
        InstructionAccount::readonly(system_program_info.address()),
    ];
    invoke_signed_with_bounds::<3>(
        &InstructionView {
            program_id: &hydra_program_id,
            accounts: &hydra_metas,
            data: &create_ix.data,
        },
        &[rent_pda_info, hydra_crank_pda_info, system_program_info],
        core::slice::from_ref(&rent_signer),
    )?;

    Transfer {
        from: rent_pda_info,
        to: hydra_crank_pda_info,
        lamports: CRANKER_REWARD,
    }
    .invoke_signed(&[rent_signer])?;

    Ok(())
}

#[inline(always)]
pub(crate) fn derive_hydra_seed(stash_pda: &Address, shuttle_id_bytes: &[u8; 4]) -> [u8; 32] {
    let mut seed = *stash_pda.as_array();
    seed[..4].copy_from_slice(shuttle_id_bytes);
    seed
}

#[inline(always)]
fn derive_ata(
    wallet: &Address,
    token_program: &Address,
    mint: &Address,
    bump_seed: u8,
) -> Result<Address, ProgramError> {
    let pda = Address::create_program_address(
        &[
            wallet.as_ref(),
            token_program.as_ref(),
            mint.as_ref(),
            &[bump_seed],
        ],
        &pinocchio_associated_token_account::ID,
    )?;
    Ok(pda)
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
