use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use dlp_api::pda::magic_fee_vault_pda_from_validator;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_spl_api::instruction::{self, internal};
use ephemeral_spl_api::state::transfer_queue::{
    header_len, item_len, QueuedTransfer, TransferQueueHeader, QUEUE_SEED,
};
use ephemeral_spl_api::ID as PROGRAM;
use magicblock_magic_program_api::{
    args::{MagicIntentBundleArgs, ScheduleTaskArgs},
    instruction::MagicBlockInstruction,
    Pubkey as MagicPubkey, MAGIC_CONTEXT_PUBKEY,
};
use solana_account::Account as SolanaAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
};
use solana_program_pack::Pack;
use spl_token_interface::state::Account;

use {
    solana_keypair::Keypair,
    solana_program_test::{processor, tokio, ProgramTest, ProgramTestContext},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

mod common;
mod utils;

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);
const QUEUED_AMOUNT: u64 = 10;
const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;
const RENT_PDA_SEED: &[u8] = b"rent";

fn automation_validator() -> Pubkey {
    utils::test_pubkey("transfer_queue_automation::validator")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedScheduleAccount {
    pubkey: Pubkey,
    is_signer: bool,
    is_writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedScheduleTask {
    schedule_accounts: Vec<CapturedScheduleAccount>,
    args: ScheduleTaskArgs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedCancelTask {
    cancel_accounts: Vec<CapturedScheduleAccount>,
    task_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapturedIntentBundle {
    schedule_accounts: Vec<Pubkey>,
    args: MagicIntentBundleArgs,
}

fn captured_schedules() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedScheduleTask>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedScheduleTask>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn captured_cancels() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedCancelTask>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedCancelTask>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn captured_intent_bundles() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clear_captured_schedules(magic_program: Pubkey) {
    captured_schedules().lock().unwrap().remove(&magic_program);
}

fn clear_captured_cancels(magic_program: Pubkey) {
    captured_cancels().lock().unwrap().remove(&magic_program);
}

fn clear_captured_intent_bundles(magic_program: Pubkey) {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .remove(&magic_program);
}

fn take_captured_schedules(magic_program: Pubkey) -> Vec<CapturedScheduleTask> {
    captured_schedules()
        .lock()
        .unwrap()
        .remove(&magic_program)
        .unwrap_or_default()
}

fn take_captured_cancels(magic_program: Pubkey) -> Vec<CapturedCancelTask> {
    captured_cancels()
        .lock()
        .unwrap()
        .remove(&magic_program)
        .unwrap_or_default()
}

fn peek_captured_intent_bundles(magic_program: Pubkey) -> Vec<CapturedIntentBundle> {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .get(&magic_program)
        .cloned()
        .unwrap_or_default()
}

fn process_magic_program_mock(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let magic_ix: MagicBlockInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match magic_ix {
        MagicBlockInstruction::ScheduleTask(args) => {
            let available_accounts: HashSet<_> = accounts
                .iter()
                .skip(1)
                .map(|account| *account.key)
                .collect();
            for instruction in &args.instructions {
                for account in &instruction.accounts {
                    let pubkey = convert_magic_pubkey(account.pubkey);
                    if !available_accounts.contains(&pubkey) {
                        return Err(ProgramError::NotEnoughAccountKeys);
                    }
                }
            }

            captured_schedules()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedScheduleTask {
                    schedule_accounts: accounts
                        .iter()
                        .map(|account| CapturedScheduleAccount {
                            pubkey: *account.key,
                            is_signer: account.is_signer,
                            is_writable: account.is_writable,
                        })
                        .collect(),
                    args,
                });
        }
        MagicBlockInstruction::CancelTask { task_id } => {
            let Some(authority) = accounts.first() else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };
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
                        .map(|account| CapturedScheduleAccount {
                            pubkey: *account.key,
                            is_signer: account.is_signer,
                            is_writable: account.is_writable,
                        })
                        .collect(),
                    task_id,
                });
        }
        MagicBlockInstruction::ScheduleIntentBundle(args) => {
            let Some(payer) = accounts.first() else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };
            if !payer.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            for action in &args.standalone_actions {
                let Some(escrow_authority) = accounts.get(action.escrow_authority as usize) else {
                    return Err(ProgramError::NotEnoughAccountKeys);
                };
                if !escrow_authority.is_signer {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }

            captured_intent_bundles()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(CapturedIntentBundle {
                    schedule_accounts: accounts.iter().map(|account| *account.key).collect(),
                    args,
                });
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }

    Ok(())
}

fn process_noop_program_mock(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    Ok(())
}

fn convert_magic_pubkey(pubkey: MagicPubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}

fn add_magic_program_mock(pt: &mut ProgramTest, magic_program: Pubkey) {
    pt.prefer_bpf(false);
    pt.add_program(
        "magic_program_mock",
        magic_program,
        processor!(process_magic_program_mock),
    );
    pt.prefer_bpf(true);
}

fn add_noop_program_mock(pt: &mut ProgramTest, program_id: Pubkey) {
    pt.prefer_bpf(false);
    pt.add_program(
        "noop_program_mock",
        program_id,
        processor!(process_noop_program_mock),
    );
    pt.prefer_bpf(true);
}

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= header_len());
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
}

fn read_item_unaligned(data: &[u8], index: usize) -> QueuedTransfer {
    let offset = header_len() + (index * item_len());
    assert!(data.len() >= offset + item_len());
    unsafe { core::ptr::read_unaligned(data[offset..].as_ptr() as *const QueuedTransfer) }
}

fn magic_fee_vault(validator: &Pubkey) -> Pubkey {
    Pubkey::new_from_array(
        magic_fee_vault_pda_from_validator(&validator.to_bytes().into()).to_bytes(),
    )
}

fn delegation_program() -> Pubkey {
    Pubkey::new_from_array(ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID.to_bytes())
}

struct Fixture {
    context: ProgramTestContext,
    payer_kp: Keypair,
    payer: Pubkey,
    mint: Pubkey,
    magic_program: Pubkey,
    magic_context: Pubkey,
    queue: Pubkey,
    magic_fee_vault: Pubkey,
    vault: Pubkey,
    vault_ata: Pubkey,
    source_ata: Pubkey,
    destination_ata: Pubkey,
}

async fn latest_blockhash(context: &mut ProgramTestContext) -> solana_program::hash::Hash {
    context.get_new_latest_blockhash().await.unwrap()
}

async fn setup_fixture() -> Fixture {
    let magic_program = solana_pubkey::Pubkey::new_from_array(
        ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes(),
    );
    let magic_context = convert_magic_pubkey(MAGIC_CONTEXT_PUBKEY);
    clear_captured_schedules(magic_program);
    clear_captured_cancels(magic_program);
    clear_captured_intent_bundles(magic_program);

    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        add_magic_program_mock(pt, magic_program);
        pt.add_account(
            magic_context,
            SolanaAccount {
                lamports: 1_000_000,
                data: vec![0; 8],
                owner: magic_program,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let mint_kp = utils::test_keypair("transfer_queue_automation::setup_fixture::mint");
    let mint = mint_kp.pubkey();
    let validator = automation_validator();
    let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

    let pdas = utils::derive_pdas(PROGRAM, payer, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        &payer_kp,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        2,
    )
    .await;

    let queue =
        Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref(), validator.as_ref()], &PROGRAM).0;
    let magic_fee_vault = magic_fee_vault(&validator);
    context.set_account(
        &magic_fee_vault,
        &SolanaAccount {
            lamports: 1_000_000,
            data: vec![],
            owner: delegation_program(),
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );
    let queue_permission = permission_pda_from_permissioned_account(&queue);
    let vault = pdas.vault;
    let source_ata = setup.user_tokens[0];
    let destination_ata = utils::derive_associated_token_address(payer, mint);
    let (vault_eata, _) = Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
    let vault_ata = utils::derive_associated_token_address(vault, mint);

    let ix_init_rent = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(rent_pda, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_RENT_PDA],
    };
    let ix_fund_rent =
        solana_system_interface::instruction::transfer(&payer, &rent_pda, 100_000_000);
    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_eata, false),
            AccountMeta::new(vault_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
    };

    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(queue_permission, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };

    let ix_init_destination_ata = Instruction {
        program_id: utils::associated_token_program_id(),
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(destination_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: vec![1],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[
            ix_init_rent,
            ix_fund_rent,
            ix_init_vault,
            ix_init_queue,
            ix_init_destination_ata,
        ],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    Fixture {
        context,
        payer_kp,
        payer,
        mint,
        magic_program,
        magic_context,
        queue,
        magic_fee_vault,
        vault,
        vault_ata,
        source_ata,
        destination_ata,
    }
}

async fn enqueue_transfer(fixture: &mut Fixture, min_delay_ms: u64) {
    enqueue_transfer_with_client_ref_id(fixture, min_delay_ms, None).await;
}

async fn enqueue_transfer_with_client_ref_id(
    fixture: &mut Fixture,
    min_delay_ms: u64,
    client_ref_id: Option<u64>,
) {
    let mut data = vec![instruction::DEPOSIT_AND_QUEUE_TRANSFER];
    data.extend_from_slice(&QUEUED_AMOUNT.to_le_bytes());
    data.extend_from_slice(&min_delay_ms.to_le_bytes());
    data.extend_from_slice(&min_delay_ms.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    if let Some(client_ref_id) = client_ref_id {
        data.extend_from_slice(&client_ref_id.to_le_bytes());
    }

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new_readonly(fixture.vault, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new(fixture.source_ata, false),
            AccountMeta::new(fixture.vault_ata, false),
            AccountMeta::new_readonly(fixture.payer, false),
            AccountMeta::new_readonly(fixture.payer, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(PROGRAM, false),
        ],
        data,
    };

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(&fixture.context.banks_client, tx, entry_key)
        .await
        .unwrap();
}

fn ensure_queue_crank_ix(fixture: &Fixture) -> Instruction {
    ensure_queue_crank_ix_with_magic_program(fixture, fixture.magic_program)
}

fn ensure_queue_crank_ix_with_magic_program(
    fixture: &Fixture,
    magic_program: Pubkey,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.payer, true),
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new(fixture.magic_fee_vault, false),
            AccountMeta::new(fixture.magic_context, false),
            AccountMeta::new_readonly(magic_program, false),
        ],
        data: vec![instruction::ENSURE_TRANSFER_QUEUE_CRANK],
    }
}

fn process_queue_tick_ix(fixture: &Fixture) -> Instruction {
    process_queue_tick_ix_with_magic_program(fixture, fixture.magic_program)
}

fn process_queue_tick_ix_with_magic_program(
    fixture: &Fixture,
    magic_program: Pubkey,
) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new(fixture.magic_fee_vault, false),
            AccountMeta::new(fixture.magic_context, false),
            AccountMeta::new_readonly(magic_program, false),
        ],
        data: vec![internal::PROCESS_TRANSFER_QUEUE_TICK],
    }
}

async fn token_amount(context: &mut ProgramTestContext, token_account: Pubkey) -> u64 {
    let account = context
        .banks_client
        .get_account(token_account)
        .await
        .unwrap()
        .expect("token account must exist");
    Account::unpack(&account.data).unwrap().amount
}

async fn queue_account(context: &mut ProgramTestContext, queue: Pubkey) -> SolanaAccount {
    context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist")
}

#[tokio::test(flavor = "current_thread")]
async fn ensure_transfer_queue_crank_schedules_one_recurring_queue_crank() {
    let _test_guard = test_lock().lock().unwrap();
    let mut fixture = setup_fixture().await;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::crank_1",
    )
    .await
    .unwrap();

    let captured = take_captured_schedules(fixture.magic_program);
    let cancels = take_captured_cancels(fixture.magic_program);
    assert_eq!(captured.len(), 1);
    assert!(cancels.is_empty());
    assert_eq!(
        captured[0].schedule_accounts,
        vec![
            CapturedScheduleAccount {
                pubkey: fixture.queue,
                is_signer: true,
                is_writable: true,
            },
            CapturedScheduleAccount {
                pubkey: fixture.queue,
                is_signer: true,
                is_writable: true,
            },
            CapturedScheduleAccount {
                pubkey: fixture.magic_fee_vault,
                is_signer: false,
                is_writable: false,
            },
            CapturedScheduleAccount {
                pubkey: fixture.magic_context,
                is_signer: false,
                is_writable: false,
            },
            CapturedScheduleAccount {
                pubkey: fixture.magic_program,
                is_signer: false,
                is_writable: false,
            },
        ]
    );
    assert_eq!(captured[0].args.execution_interval_millis, 500);
    assert_eq!(captured[0].args.iterations, i64::MAX);
    assert_eq!(captured[0].args.instructions.len(), 1);
    assert_eq!(
        captured[0].args.instructions[0].program_id.to_bytes(),
        PROGRAM.to_bytes()
    );
    assert_eq!(captured[0].args.instructions[0].accounts.len(), 4);
    assert_eq!(
        captured[0].args.instructions[0].accounts[0]
            .pubkey
            .to_bytes(),
        fixture.queue.to_bytes()
    );
    assert_eq!(
        captured[0].args.instructions[0].accounts[1]
            .pubkey
            .to_bytes(),
        fixture.magic_fee_vault.to_bytes()
    );
    assert_eq!(
        captured[0].args.instructions[0].accounts[2]
            .pubkey
            .to_bytes(),
        fixture.magic_context.to_bytes()
    );
    assert_eq!(
        captured[0].args.instructions[0].accounts[3]
            .pubkey
            .to_bytes(),
        fixture.magic_program.to_bytes()
    );
    assert_eq!(
        captured[0].args.instructions[0].data,
        vec![internal::PROCESS_TRANSFER_QUEUE_TICK]
    );
    let queue_after_first_ensure = queue_account(&mut fixture.context, fixture.queue).await;
    let header_after_first_ensure = read_header_unaligned(&queue_after_first_ensure.data);
    assert_eq!(
        header_after_first_ensure.crank_task_id,
        captured[0].args.task_id
    );

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::crank_2",
    )
    .await
    .unwrap();
    let cancels = take_captured_cancels(fixture.magic_program);
    assert_eq!(cancels.len(), 1);
    assert_eq!(cancels[0].task_id, header_after_first_ensure.crank_task_id);
    assert_eq!(
        cancels[0].cancel_accounts,
        vec![
            CapturedScheduleAccount {
                pubkey: fixture.queue,
                is_signer: true,
                is_writable: true,
            },
            CapturedScheduleAccount {
                pubkey: fixture.queue,
                is_signer: true,
                is_writable: true,
            },
        ]
    );
    let rescheduled = take_captured_schedules(fixture.magic_program);
    assert_eq!(rescheduled.len(), 1);
    assert_eq!(
        rescheduled[0].args.task_id,
        header_after_first_ensure.crank_task_id
    );
}

#[tokio::test(flavor = "current_thread")]
async fn ensure_transfer_queue_crank_rejects_non_magic_program() {
    let _test_guard = test_lock().lock().unwrap();
    let fake_magic_program = utils::test_pubkey("transfer_queue_automation::fake_magic_program");

    let mut fixture = {
        let magic_program = solana_pubkey::Pubkey::new_from_array(
            ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes(),
        );
        let magic_context = convert_magic_pubkey(MAGIC_CONTEXT_PUBKEY);
        clear_captured_schedules(magic_program);
        clear_captured_cancels(magic_program);
        clear_captured_intent_bundles(magic_program);

        let mut context = utils::start_program_test_with(PROGRAM, |pt| {
            add_magic_program_mock(pt, magic_program);
            add_noop_program_mock(pt, fake_magic_program);
            pt.add_account(
                magic_context,
                SolanaAccount {
                    lamports: 1_000_000,
                    data: vec![0; 8],
                    owner: magic_program,
                    executable: false,
                    rent_epoch: 0,
                },
            );
        })
        .await;

        let payer_kp = utils::fixed_payer_keypair();
        let payer = payer_kp.pubkey();
        let mint_kp = utils::test_keypair(
            "transfer_queue_automation::ensure_transfer_queue_crank_rejects_non_magic_program::mint",
        );
        let mint = mint_kp.pubkey();
        let validator = automation_validator();
        let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

        let pdas = utils::derive_pdas(PROGRAM, payer, mint);
        let setup = utils::setup_mint_and_token_accounts(
            &mut context,
            &payer_kp,
            &mint_kp,
            DECIMALS,
            STARTING_BALANCE,
            2,
        )
        .await;

        let queue = Pubkey::find_program_address(
            &[QUEUE_SEED, mint.as_ref(), validator.as_ref()],
            &PROGRAM,
        )
        .0;
        let magic_fee_vault = magic_fee_vault(&validator);
        context.set_account(
            &magic_fee_vault,
            &SolanaAccount {
                lamports: 1_000_000,
                data: vec![],
                owner: delegation_program(),
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );
        let queue_permission = permission_pda_from_permissioned_account(&queue);
        let vault = pdas.vault;
        let source_ata = setup.user_tokens[0];
        let destination_ata = utils::derive_associated_token_address(payer, mint);
        let (vault_eata, _) =
            Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
        let vault_ata = utils::derive_associated_token_address(vault, mint);

        let ix_init_rent = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(rent_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![instruction::INITIALIZE_RENT_PDA],
        };
        let ix_fund_rent =
            solana_system_interface::instruction::transfer(&payer, &rent_pda, 100_000_000);
        let ix_init_vault = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(vault, false),
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(vault_eata, false),
                AccountMeta::new(vault_ata, false),
                AccountMeta::new_readonly(spl_token_interface::ID, false),
                AccountMeta::new_readonly(utils::associated_token_program_id(), false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
        };

        let ix_init_queue = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(queue, false),
                AccountMeta::new(queue_permission, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(validator, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
            ],
            data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
        };

        let ix_init_destination_ata = Instruction {
            program_id: utils::associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(destination_ata, false),
                AccountMeta::new_readonly(payer, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(spl_token_interface::ID, false),
            ],
            data: vec![1],
        };

        let tx_init = Transaction::new_signed_with_payer(
            &[
                ix_init_rent,
                ix_fund_rent,
                ix_init_vault,
                ix_init_queue,
                ix_init_destination_ata,
            ],
            Some(&payer),
            &[&payer_kp],
            context.last_blockhash,
        );
        context
            .banks_client
            .process_transaction(tx_init)
            .await
            .unwrap();

        Fixture {
            context,
            payer_kp,
            payer,
            mint,
            magic_program,
            magic_context,
            queue,
            magic_fee_vault,
            vault,
            vault_ata,
            source_ata,
            destination_ata,
        }
    };

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix_with_magic_program(
            &fixture,
            fake_magic_program,
        )],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let r = common::metrics::process_transaction_with_metadata_recorded(
        &fixture.context.banks_client,
        tx,
        "tq_auto::crank_bad_magic",
    )
    .await
    .unwrap();
    assert_eq!(
        r.result.unwrap_err(),
        solana_transaction::TransactionError::InstructionError(
            0,
            solana_program::instruction::InstructionError::IncorrectProgramId,
        )
    );

    let queue_after = queue_account(&mut fixture.context, fixture.queue).await;
    let header = read_header_unaligned(&queue_after.data);
    assert_eq!(header.crank_task_id, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn process_transfer_queue_tick_is_noop_when_queue_is_empty() {
    let _test_guard = test_lock().lock().unwrap();
    let mut fixture = setup_fixture().await;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::tick_empty",
    )
    .await
    .unwrap();

    assert!(peek_captured_intent_bundles(fixture.magic_program).is_empty());
    let header = read_header_unaligned(
        &queue_account(&mut fixture.context, fixture.queue)
            .await
            .data,
    );
    assert_eq!(header.length, 0);
    assert_eq!(
        token_amount(&mut fixture.context, fixture.vault_ata).await,
        0
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.destination_ata).await,
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn process_transfer_queue_tick_rejects_non_magic_program() {
    let _test_guard = test_lock().lock().unwrap();
    let fake_magic_program = utils::test_pubkey(
        "transfer_queue_automation::process_transfer_queue_tick_rejects_non_magic_program::fake_magic",
    );

    let mut fixture = {
        let magic_program = solana_pubkey::Pubkey::new_from_array(
            ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes(),
        );
        let magic_context = convert_magic_pubkey(MAGIC_CONTEXT_PUBKEY);
        clear_captured_schedules(magic_program);
        clear_captured_cancels(magic_program);
        clear_captured_intent_bundles(magic_program);

        let mut context = utils::start_program_test_with(PROGRAM, |pt| {
            add_magic_program_mock(pt, magic_program);
            add_noop_program_mock(pt, fake_magic_program);
            pt.add_account(
                magic_context,
                SolanaAccount {
                    lamports: 1_000_000,
                    data: vec![0; 8],
                    owner: magic_program,
                    executable: false,
                    rent_epoch: 0,
                },
            );
        })
        .await;

        let payer_kp = utils::fixed_payer_keypair();
        let payer = payer_kp.pubkey();
        let mint_kp = utils::test_keypair(
            "transfer_queue_automation::process_transfer_queue_tick_rejects_non_magic_program::mint",
        );
        let mint = mint_kp.pubkey();
        let validator = automation_validator();
        let (rent_pda, _) = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM);

        let pdas = utils::derive_pdas(PROGRAM, payer, mint);
        let setup = utils::setup_mint_and_token_accounts(
            &mut context,
            &payer_kp,
            &mint_kp,
            DECIMALS,
            STARTING_BALANCE,
            2,
        )
        .await;

        let queue = Pubkey::find_program_address(
            &[QUEUE_SEED, mint.as_ref(), validator.as_ref()],
            &PROGRAM,
        )
        .0;
        let queue_permission = permission_pda_from_permissioned_account(&queue);
        let magic_fee_vault = magic_fee_vault(&validator);
        context.set_account(
            &magic_fee_vault,
            &SolanaAccount {
                lamports: 1_000_000,
                data: vec![],
                owner: delegation_program(),
                executable: false,
                rent_epoch: 0,
            }
            .into(),
        );
        let vault = pdas.vault;
        let source_ata = setup.user_tokens[0];
        let destination_ata = utils::derive_associated_token_address(payer, mint);
        let (vault_eata, _) =
            Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
        let vault_ata = utils::derive_associated_token_address(vault, mint);

        let ix_init_rent = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(rent_pda, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![instruction::INITIALIZE_RENT_PDA],
        };
        let ix_fund_rent =
            solana_system_interface::instruction::transfer(&payer, &rent_pda, 100_000_000);
        let ix_init_vault = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(vault, false),
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(vault_eata, false),
                AccountMeta::new(vault_ata, false),
                AccountMeta::new_readonly(spl_token_interface::ID, false),
                AccountMeta::new_readonly(utils::associated_token_program_id(), false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![instruction::INITIALIZE_GLOBAL_VAULT],
        };

        let ix_init_queue = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(queue, false),
                AccountMeta::new(queue_permission, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(validator, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
            ],
            data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
        };

        let ix_init_destination_ata = Instruction {
            program_id: utils::associated_token_program_id(),
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(destination_ata, false),
                AccountMeta::new_readonly(payer, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(spl_token_interface::ID, false),
            ],
            data: vec![1],
        };

        let tx_init = Transaction::new_signed_with_payer(
            &[
                ix_init_rent,
                ix_fund_rent,
                ix_init_vault,
                ix_init_queue,
                ix_init_destination_ata,
            ],
            Some(&payer),
            &[&payer_kp],
            context.last_blockhash,
        );
        context
            .banks_client
            .process_transaction(tx_init)
            .await
            .unwrap();

        Fixture {
            context,
            payer_kp,
            payer,
            mint,
            magic_program,
            magic_context,
            queue,
            magic_fee_vault,
            vault,
            vault_ata,
            source_ata,
            destination_ata,
        }
    };

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix_with_magic_program(
            &fixture,
            fake_magic_program,
        )],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    let err = fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        solana_transaction::TransactionError::InstructionError(
            0,
            solana_program::instruction::InstructionError::IncorrectProgramId,
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn process_transfer_queue_tick_is_noop_when_next_transfer_is_not_ready() {
    let _test_guard = test_lock().lock().unwrap();
    let mut fixture = setup_fixture().await;
    enqueue_transfer(&mut fixture, 120, "tq_auto::enqueue_delayed").await;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::tick_not_ready",
    )
    .await
    .unwrap();

    assert!(peek_captured_intent_bundles(fixture.magic_program).is_empty());
    let header = read_header_unaligned(
        &queue_account(&mut fixture.context, fixture.queue)
            .await
            .data,
    );
    assert_eq!(header.length, 1);
    assert_eq!(
        token_amount(&mut fixture.context, fixture.vault_ata).await,
        QUEUED_AMOUNT
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.destination_ata).await,
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn recurring_queue_crank_executes_ready_transfer_via_magic_bundle() {
    let _test_guard = test_lock().lock().unwrap();
    let mut fixture = setup_fixture().await;
    enqueue_transfer(&mut fixture, 0, "tq_auto::enqueue_ready").await;

    let queue_before = queue_account(&mut fixture.context, fixture.queue).await;
    let queued = read_item_unaligned(&queue_before.data, 0);
    let expected_amount = queued.amount;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::schedule",
    )
    .await
    .unwrap();

    let captured = take_captured_schedules(fixture.magic_program);
    assert_eq!(captured.len(), 1);

    let scheduled_ix = Instruction {
        program_id: convert_magic_pubkey(captured[0].args.instructions[0].program_id),
        accounts: captured[0].args.instructions[0]
            .accounts
            .iter()
            .map(|meta| AccountMeta {
                pubkey: convert_magic_pubkey(meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: captured[0].args.instructions[0].data.clone(),
    };
    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[scheduled_ix],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::run_scheduled_tick",
    )
    .await
    .unwrap();

    let captured_bundles = peek_captured_intent_bundles(fixture.magic_program);
    assert_eq!(captured_bundles.len(), 1);
    assert_eq!(captured_bundles[0].args.standalone_actions.len(), 1);
    assert_eq!(
        captured_bundles[0].schedule_accounts,
        vec![
            fixture.queue,
            fixture.magic_context,
            fixture.magic_fee_vault
        ]
    );

    let queue_after_scheduling = queue_account(&mut fixture.context, fixture.queue).await;
    let header_after_scheduling = read_header_unaligned(&queue_after_scheduling.data);
    assert_eq!(header_after_scheduling.length, 0);

    let standalone_action = &captured_bundles[0].args.standalone_actions[0];
    assert_eq!(
        standalone_action.destination_program.to_bytes(),
        PROGRAM.to_bytes()
    );
    assert_eq!(standalone_action.compute_units, 140_000);
    assert_eq!(
        captured_bundles[0].schedule_accounts[standalone_action.escrow_authority as usize],
        fixture.queue
    );
    assert_eq!(
        standalone_action.args.escrow_index,
        EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX
    );
    let mut expected_action_data = vec![
        internal::EXECUTE_READY_QUEUED_TRANSFER,
        EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX,
    ];
    expected_action_data.extend_from_slice(&expected_amount.to_le_bytes());
    expected_action_data
        .push(ephemeral_spl_api::state::transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA);
    assert_eq!(standalone_action.args.data, expected_action_data);
    assert_eq!(standalone_action.accounts.len(), 9);
    let rent_pda = Pubkey::find_program_address(&[RENT_PDA_SEED], &PROGRAM).0;
    assert_eq!(
        standalone_action.accounts[0].pubkey.to_bytes(),
        fixture.vault.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[1].pubkey.to_bytes(),
        fixture.mint.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[2].pubkey.to_bytes(),
        fixture.vault_ata.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[3].pubkey.to_bytes(),
        fixture.payer.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[4].pubkey.to_bytes(),
        fixture.destination_ata.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[5].pubkey.to_bytes(),
        rent_pda.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[6].pubkey.to_bytes(),
        spl_token_interface::ID.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[7].pubkey.to_bytes(),
        utils::associated_token_program_id().to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[8].pubkey.to_bytes(),
        solana_system_interface::program::ID.to_bytes()
    );

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.payer_kp],
        blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &fixture.context.banks_client,
        tx,
        "tq_auto::tick_after_bundle",
    )
    .await
    .unwrap();
    assert_eq!(peek_captured_intent_bundles(fixture.magic_program).len(), 1);

    let queue_after = queue_account(&mut fixture.context, fixture.queue).await;
    let header_after = read_header_unaligned(&queue_after.data);
    assert_eq!(header_after.length, 0);
    assert_eq!(
        token_amount(&mut fixture.context, fixture.source_ata).await,
        STARTING_BALANCE - QUEUED_AMOUNT
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.vault_ata).await,
        QUEUED_AMOUNT
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.destination_ata).await,
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn recurring_queue_crank_includes_client_ref_id_in_execute_action_when_present() {
    let _test_guard = test_lock().lock().unwrap();
    let mut fixture = setup_fixture().await;
    enqueue_transfer_with_client_ref_id(&mut fixture, 0, Some(42)).await;

    let queue_before = queue_account(&mut fixture.context, fixture.queue).await;
    let queued = read_item_unaligned(&queue_before.data, 0);
    let expected_amount = queued.amount;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix(&fixture)],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    let captured = take_captured_schedules(fixture.magic_program);
    assert_eq!(captured.len(), 1);

    let scheduled_ix = Instruction {
        program_id: convert_magic_pubkey(captured[0].args.instructions[0].program_id),
        accounts: captured[0].args.instructions[0]
            .accounts
            .iter()
            .map(|meta| AccountMeta {
                pubkey: convert_magic_pubkey(meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: captured[0].args.instructions[0].data.clone(),
    };
    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[scheduled_ix],
        Some(&fixture.payer),
        &[&fixture.context.payer],
        blockhash,
    );
    fixture
        .context
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    let captured_bundles = peek_captured_intent_bundles(fixture.magic_program);
    assert_eq!(captured_bundles.len(), 1);
    assert_eq!(captured_bundles[0].args.standalone_actions.len(), 1);

    let transfer_action = &captured_bundles[0].args.standalone_actions[0];
    assert_eq!(
        transfer_action.destination_program.to_bytes(),
        PROGRAM.to_bytes()
    );
    let mut expected_action_data = vec![
        internal::EXECUTE_READY_QUEUED_TRANSFER,
        EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX,
    ];
    expected_action_data.extend_from_slice(&expected_amount.to_le_bytes());
    expected_action_data
        .push(ephemeral_spl_api::state::transfer_queue::QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA);
    expected_action_data.extend_from_slice(&42_u64.to_le_bytes());
    assert_eq!(transfer_action.args.data, expected_action_data);
}
