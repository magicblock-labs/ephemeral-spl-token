use core::mem::MaybeUninit;

use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::crank::{
    CancelCrankCpi, CrankInstruction, ScheduleCrankArgs, ScheduleCrankCpi,
};
use ephemeral_spl_api::instruction::internal::PROCESS_TRANSFER_QUEUE_TICK;
use ephemeral_spl_api::state::transfer_queue::{
    queue_crank_task_id_from_data, queue_set_crank_task_id_from_data, queue_views_checked,
    TransferQueue,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::cpi::{invoke_signed_with_bounds, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

pub const CRANK_EXECUTION_INTERVAL_MILLIS: i64 = 500;

const PROCESS_QUEUE_TICK_CRANK_ACCOUNTS: usize = 4;
const SCHEDULE_CRANK_CPI_ACCOUNTS: usize = 5;
const SCHEDULE_CRANK_DATA_LEN: usize =
    4 + 8 + 8 + 8 + 8 + 32 + 8 + (PROCESS_QUEUE_TICK_CRANK_ACCOUNTS * 34) + 8 + 1;

///
/// Executes on:
///
/// Accounts:
///
///  0: [writable, signer]  - Keypair : Payer for the recurring crank.
///  1: [writable]          - PDA     : Transfer queue account (PDA derived from [QUEUE_SEED, mint, validator]).
///  2: [writable]          - PDA     : Validator magic fee vault PDA derived from ["magic-fee-vault", validator].
///  3: [writable]          - Any     : Magic context account.
///  4: []                  - Program : Magic program.
///
/// Instruction Data: None
///
#[inline(always)]
pub fn process_ensure_transfer_queue_crank(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    ] = require_n_accounts!(accounts, 5);

    require!(
        instruction_data.is_empty(),
        ProgramError::InvalidInstructionData
    );

    // TODO (snawaz): re-review this!
    //
    // why do we require payer_info (as signer) if it is not used anywhere?
    // in the downstream CPI, we use queue_info as authority, that makes queue automation effectively
    // permissionless (means, literally anyone can invoke it).
    //
    // What if attackers repeatedly invoke this current ix?

    require!(
        payer_info.is_signer(),
        ProgramError::MissingRequiredSignature
    );
    require!(
        queue_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    require_eq_keys!(
        magic_program_info.address(),
        &ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID,
        ProgramError::IncorrectProgramId
    );

    let (mint, bump, validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        (header.mint, header.bump, header.validator)
    };

    let derived_queue = TransferQueue::derive_pda(&mint, &validator, bump)?;
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );
    let derived_magic_fee_vault = magic_fee_vault_pda_from_validator(&validator.to_bytes().into());
    require!(
        derived_magic_fee_vault.to_bytes() == magic_fee_vault_info.address().to_bytes(),
        ProgramError::InvalidSeeds
    );

    let bump_seed = [bump];
    let queue_signer_seeds = TransferQueue::signer_seeds(&mint, &validator, &bump_seed);
    let queue_signers = [Signer::from(&queue_signer_seeds)];

    let crank_task_id = derive_queue_crank_task_id(queue_info.address());
    let data = unsafe { queue_info.borrow_unchecked() };
    if let Some(existing_task_id) = queue_crank_task_id_from_data(data)? {
        // TODO (snawaz): once we have a way to know the crank status, conditionally
        // apply this this "cancel-and-reschedule" strategy:
        //  - if crank is running: return early
        //  - else: cancel-and-reschedule
        CancelCrankCpi {
            authority: queue_info.clone(),
            task_context: queue_info.clone(),
            magic_program: magic_program_info.clone(),
            crank_id: existing_task_id,
        }
        .invoke_signed(&queue_signers)?;
    }

    let tick_data = [PROCESS_TRANSFER_QUEUE_TICK];
    let tick_accounts = [
        InstructionAccount {
            address: queue_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: magic_fee_vault_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: magic_context_info.address(),
            is_signer: false,
            is_writable: true,
        },
        InstructionAccount {
            address: magic_program_info.address(),
            is_signer: false,
            is_writable: false,
        },
    ];
    let crank_instruction = [CrankInstruction::new(crate::ID, &tick_accounts, &tick_data)];
    let mut crank_data = [0_u8; SCHEDULE_CRANK_DATA_LEN];
    let schedule_cpi = ScheduleCrankCpi::new(
        queue_info.clone(),
        magic_program_info.clone(),
        &[],
        ScheduleCrankArgs::new(crank_task_id, &crank_instruction)
            .execution_interval_millis(CRANK_EXECUTION_INTERVAL_MILLIS)
            .iterations(i64::MAX),
    );
    let data_len = schedule_cpi.serialize_into(&mut crank_data)?;

    let mut schedule_accounts =
        [const { MaybeUninit::<InstructionAccount>::uninit() }; SCHEDULE_CRANK_CPI_ACCOUNTS];
    unsafe {
        // TODO (snawaz): re-review this.
        //
        // this ix is effectively permissionless (payer_info can be any
        // signer), but the downstream Magic ScheduleTask/CancelTask authority is
        // `queue_info`, signed by this program via PDA seeds. so any caller can proxy
        // queue-authorized crank management through this ix.
        schedule_accounts
            .get_unchecked_mut(0)
            .write(InstructionAccount::writable_signer(queue_info.address()));
        schedule_accounts
            .get_unchecked_mut(1)
            .write(InstructionAccount::readonly(queue_info.address()));
        schedule_accounts
            .get_unchecked_mut(2)
            .write(InstructionAccount::readonly(magic_fee_vault_info.address()));
        schedule_accounts
            .get_unchecked_mut(3)
            .write(InstructionAccount::readonly(magic_context_info.address()));
        schedule_accounts
            .get_unchecked_mut(4)
            .write(InstructionAccount::readonly(magic_program_info.address()));
    }

    let schedule_instruction = InstructionView {
        program_id: magic_program_info.address(),
        data: &crank_data[..data_len],
        accounts: unsafe {
            core::slice::from_raw_parts(
                schedule_accounts.as_ptr() as *const InstructionAccount,
                SCHEDULE_CRANK_CPI_ACCOUNTS,
            )
        },
    };
    let schedule_account_refs = [
        queue_info,
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    ];
    invoke_signed_with_bounds::<SCHEDULE_CRANK_CPI_ACCOUNTS>(
        &schedule_instruction,
        &schedule_account_refs,
        &queue_signers,
    )?;

    let data = unsafe { queue_info.borrow_unchecked_mut() };
    queue_set_crank_task_id_from_data(data, crank_task_id)?;
    Ok(())
}

//
// TODO (perf): avoid loop, copies, etc.
//
#[inline(always)]
pub(crate) fn derive_queue_crank_task_id(queue_address: &ephemeral_spl_api::Address) -> i64 {
    let mut acc = 0_u64;
    for chunk in queue_address.as_ref().chunks_exact(8) {
        let mut bytes = [0_u8; 8];
        bytes.copy_from_slice(chunk);
        acc ^= u64::from_le_bytes(bytes);
    }
    acc &= i64::MAX as u64;
    if acc == 0 {
        1
    } else {
        acc as i64
    }
}
