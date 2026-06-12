#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use magicblock_magic_program_api::{
    args::{AddActionCallbackArgs, MagicIntentBundleArgs, ScheduleTaskArgs},
    instruction::MagicBlockInstruction,
};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
};
use solana_program_test::ProgramTest;
use solana_pubkey::Pubkey;
// ── Captured types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedScheduleAccount {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedScheduleTask {
    pub schedule_accounts: Vec<CapturedScheduleAccount>,
    pub args: ScheduleTaskArgs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCancelTask {
    pub cancel_accounts: Vec<CapturedScheduleAccount>,
    pub task_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedIntentBundle {
    pub schedule_accounts: Vec<Pubkey>,
    pub args: MagicIntentBundleArgs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedActionCallback {
    pub accounts: Vec<Pubkey>,
    pub args: AddActionCallbackArgs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCreateEphemeralAccount {
    pub accounts: Vec<Pubkey>,
    pub data_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedResizeEphemeralAccount {
    pub accounts: Vec<Pubkey>,
    pub new_data_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCloseEphemeralAccount {
    pub accounts: Vec<Pubkey>,
}

// ── Global state ──────────────────────────────────────────────────────────────

fn captured_schedules() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedScheduleTask>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedScheduleTask>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_cancels() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedCancelTask>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedCancelTask>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_intent_bundles() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_action_callbacks() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedActionCallback>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedActionCallback>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_ephemeral_creates(
) -> &'static Mutex<HashMap<Pubkey, Vec<CapturedCreateEphemeralAccount>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedCreateEphemeralAccount>>>> =
        OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_ephemeral_resizes(
) -> &'static Mutex<HashMap<Pubkey, Vec<CapturedResizeEphemeralAccount>>> {
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedResizeEphemeralAccount>>>> =
        OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_ephemeral_closes() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedCloseEphemeralAccount>>>
{
    static S: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedCloseEphemeralAccount>>>> =
        OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── clear / take / peek helpers ───────────────────────────────────────────────

pub fn clear_captured_schedules(program: Pubkey) {
    captured_schedules().lock().unwrap().remove(&program);
}

pub fn clear_captured_cancels(program: Pubkey) {
    captured_cancels().lock().unwrap().remove(&program);
}

pub fn clear_captured_intent_bundles(program: Pubkey) {
    captured_intent_bundles().lock().unwrap().remove(&program);
}

pub fn clear_all_captured(program: Pubkey) {
    captured_schedules().lock().unwrap().remove(&program);
    captured_cancels().lock().unwrap().remove(&program);
    captured_intent_bundles().lock().unwrap().remove(&program);
    captured_action_callbacks().lock().unwrap().remove(&program);
    captured_ephemeral_creates()
        .lock()
        .unwrap()
        .remove(&program);
    captured_ephemeral_resizes()
        .lock()
        .unwrap()
        .remove(&program);
    captured_ephemeral_closes().lock().unwrap().remove(&program);
}

pub fn take_captured_schedules(program: Pubkey) -> Vec<CapturedScheduleTask> {
    captured_schedules()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn peek_captured_schedules(program: Pubkey) -> Vec<CapturedScheduleTask> {
    captured_schedules()
        .lock()
        .unwrap()
        .get(&program)
        .cloned()
        .unwrap_or_default()
}

pub fn take_captured_cancels(program: Pubkey) -> Vec<CapturedCancelTask> {
    captured_cancels()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn peek_captured_cancels(program: Pubkey) -> Vec<CapturedCancelTask> {
    captured_cancels()
        .lock()
        .unwrap()
        .get(&program)
        .cloned()
        .unwrap_or_default()
}

pub fn take_captured_intent_bundles(program: Pubkey) -> Vec<CapturedIntentBundle> {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn peek_captured_intent_bundles(program: Pubkey) -> Vec<CapturedIntentBundle> {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .get(&program)
        .cloned()
        .unwrap_or_default()
}

pub fn take_captured_action_callbacks(program: Pubkey) -> Vec<CapturedActionCallback> {
    captured_action_callbacks()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn take_captured_ephemeral_creates(program: Pubkey) -> Vec<CapturedCreateEphemeralAccount> {
    captured_ephemeral_creates()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn take_captured_ephemeral_resizes(program: Pubkey) -> Vec<CapturedResizeEphemeralAccount> {
    captured_ephemeral_resizes()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

pub fn take_captured_ephemeral_closes(program: Pubkey) -> Vec<CapturedCloseEphemeralAccount> {
    captured_ephemeral_closes()
        .lock()
        .unwrap()
        .remove(&program)
        .unwrap_or_default()
}

// ── Mock processor ─────────────────────────────────────────────────────────────

pub fn process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let ix: MagicBlockInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match ix {
        MagicBlockInstruction::ScheduleTask(args) => {
            captured_schedules()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedScheduleTask {
                    schedule_accounts: accounts
                        .iter()
                        .map(|a| CapturedScheduleAccount {
                            pubkey: *a.key,
                            is_signer: a.is_signer,
                            is_writable: a.is_writable,
                        })
                        .collect(),
                    args,
                });
        }
        MagicBlockInstruction::CancelTask { task_id } => {
            let authority = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
            if !authority.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            captured_cancels()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedCancelTask {
                    cancel_accounts: accounts
                        .iter()
                        .map(|a| CapturedScheduleAccount {
                            pubkey: *a.key,
                            is_signer: a.is_signer,
                            is_writable: a.is_writable,
                        })
                        .collect(),
                    task_id,
                });
        }
        MagicBlockInstruction::ScheduleIntentBundle(args) => {
            let payer = accounts.first().ok_or(ProgramError::NotEnoughAccountKeys)?;
            if !payer.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            for action in &args.standalone_actions {
                let escrow = accounts
                    .get(action.escrow_authority as usize)
                    .ok_or(ProgramError::NotEnoughAccountKeys)?;
                if !escrow.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }
            captured_intent_bundles()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedIntentBundle {
                    schedule_accounts: accounts.iter().map(|a| *a.key).collect(),
                    args,
                });
        }
        MagicBlockInstruction::AddActionCallback(args) => {
            captured_action_callbacks()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedActionCallback {
                    accounts: accounts.iter().map(|a| *a.key).collect(),
                    args,
                });
        }
        MagicBlockInstruction::CreateEphemeralAccount { data_len } => {
            captured_ephemeral_creates()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedCreateEphemeralAccount {
                    accounts: accounts.iter().map(|a| *a.key).collect(),
                    data_len,
                });
        }
        MagicBlockInstruction::ResizeEphemeralAccount { new_data_len } => {
            captured_ephemeral_resizes()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedResizeEphemeralAccount {
                    accounts: accounts.iter().map(|a| *a.key).collect(),
                    new_data_len,
                });
        }
        MagicBlockInstruction::CloseEphemeralAccount => {
            captured_ephemeral_closes()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedCloseEphemeralAccount {
                    accounts: accounts.iter().map(|a| *a.key).collect(),
                });
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }

    Ok(())
}

// ── ProgramTest registration ──────────────────────────────────────────────────

pub fn add_mock(pt: &mut ProgramTest) {
    use solana_program_test::processor;
    pt.prefer_bpf(false);
    pt.add_program("magic_mock", MAGIC_PROGRAM_ID, processor!(process));
    pt.prefer_bpf(true);
}
