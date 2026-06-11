use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use wheels::layout::{Decodable as _, Encodable as _};

use ephemeral_rollups_pinocchio::consts::{
    BUFFER, DELEGATION_METADATA, DELEGATION_PROGRAM_ID, DELEGATION_RECORD,
};
use ephemeral_spl_api::instruction::ESplInstruction;
use ephemeral_spl_api::instructions::{
    ExecuteScheduledPrivateTransferArgs, SchedulePrivateTransferArgs,
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

use crate::processor::internal::private_transfer::SCHEDULED_PT_ACCOUNTS;
use crate::processor::internal::rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED};
use crate::processor::internal::{derive_ata, derive_hydra_seed};
use crate::processor::internal::{get_associated_token_address, is_supported_token_program};

const SETUP_LAMPORTS: u64 = ephemeral_spl_api::consts::SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS
    + ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS;

///
/// Executes on: BASE only. User-signed.
///
/// Appended to a swap transaction to schedule a private transfer
/// (instruction 31 via Hydra) over whatever balance ends up in the stash
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
/// Instruction Data: SchedulePrivateTransferArgs
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

    let args = SchedulePrivateTransferArgs::decode(instruction_data)?;

    let derived_stash = StashPda::derive_pda(user_info.address(), args.mint(), args.stash_bump())?;
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
        args.mint(),
        args.shuttle_id(),
        args.shuttle_bump(),
    )?;
    let shuttle_eata = EphemeralAta::derive_pda(&shuttle, args.mint(), args.shuttle_eata_bump())?;
    let shuttle_wallet_ata = derive_ata(
        &shuttle,
        &token_program_id,
        args.mint(),
        args.shuttle_wallet_ata_bump(),
    )?;
    let buffer = Address::create_program_address(
        &[BUFFER, shuttle_eata.as_ref(), &[args.buffer_bump()]],
        &crate::ID,
    )?;
    let delegation_record = Address::create_program_address(
        &[
            DELEGATION_RECORD,
            shuttle_eata.as_ref(),
            &[args.delegation_record_bump()],
        ],
        &DELEGATION_PROGRAM_ID,
    )?;
    let delegation_metadata = Address::create_program_address(
        &[
            DELEGATION_METADATA,
            shuttle_eata.as_ref(),
            &[args.delegation_metadata_bump()],
        ],
        &DELEGATION_PROGRAM_ID,
    )?;
    let global_vault = GlobalVault::derive_pda(args.mint(), args.global_vault_bump())?;
    let vault_token = derive_ata(
        &global_vault,
        &token_program_id,
        args.mint(),
        args.vault_token_bump(),
    )?;
    let stash_ata = derive_ata(
        stash_pda_info.address(),
        &token_program_id,
        args.mint(),
        args.stash_ata_bump(),
    )?;
    let user_ata =
        get_associated_token_address(user_info.address(), args.mint(), &token_program_id);
    let queue = TransferQueue::derive_pda(args.mint(), args.validator(), args.queue_bump())?;

    // Slots 0..18 mirror ix 31's layout. Slot 5 aliases slot 0 (stash PDA).
    // Slot 20 aliases Trigger's crank account; the flag must match Solana's
    // tx-level writable union, so it remains writable.
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
        (args.mint(), false),
        (&token_program_id, false),
        (&global_vault, false),
        (&stash_ata, true),
        (&vault_token, true),
        (&queue, true),
        (&user_ata, true),
        (hydra_crank_pda_info.address(), true),
    ];

    let hydra_program_id = Address::new_from_array(hydra_api::ID.to_bytes());
    let hydra_seed = derive_hydra_seed(stash_pda_info.address(), args.shuttle_id());
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
            scheduled_data: &ESplInstruction::ExecuteScheduledPrivateTransfer.with_data(
                &ExecuteScheduledPrivateTransferArgs {
                    user: *user_info.address(),
                    stash_bump: args.stash_bump(),
                    shuttle_id: args.shuttle_id(),
                    validator: *args.validator(),
                    encrypted_destination: args.encrypted_destination().to_owned(),
                    encrypted_data_suffix: args.encrypted_data_suffix().to_owned(),
                }
                .encode()?,
            ),
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
    .invoke_signed(&[rent_signer])
}
