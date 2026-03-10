use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use ephemeral_spl_api::instruction::{self, internal};
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    header_len, item_len, QueuedTransfer, TransferQueueHeader, QUEUE_SEED,
};
use magicblock_magic_program_api::{
    args::{MagicIntentBundleArgs, ScheduleTaskArgs},
    instruction::MagicBlockInstruction,
    Pubkey as MagicPubkey,
};
use solana_account::{Account as SolanaAccount, AccountSharedData};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program::invoke_signed,
    program_error::ProgramError, rent::Rent,
};
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {
    solana_program_test::{processor, tokio, ProgramTest, ProgramTestContext},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);
const QUEUED_AMOUNT: u64 = 10;
const EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX: u8 = 0;
const EXECUTE_CAPTURED_STANDALONE_ACTION: &[u8] = &[250];

#[derive(Clone)]
struct CapturedIntentBundle {
    schedule_accounts: Vec<Pubkey>,
    args: MagicIntentBundleArgs,
}

fn captured_schedules() -> &'static Mutex<HashMap<Pubkey, Vec<ScheduleTaskArgs>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<ScheduleTaskArgs>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn captured_intent_bundles() -> &'static Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>> {
    static CAPTURED: OnceLock<Mutex<HashMap<Pubkey, Vec<CapturedIntentBundle>>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clear_captured_schedules(magic_program: Pubkey) {
    captured_schedules().lock().unwrap().remove(&magic_program);
}

fn clear_captured_intent_bundles(magic_program: Pubkey) {
    captured_intent_bundles()
        .lock()
        .unwrap()
        .remove(&magic_program);
}

fn take_captured_schedules(magic_program: Pubkey) -> Vec<ScheduleTaskArgs> {
    captured_schedules()
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

fn pop_captured_intent_bundle(magic_program: Pubkey) -> Option<CapturedIntentBundle> {
    let mut captured = captured_intent_bundles().lock().unwrap();
    let bundle = captured.get_mut(&magic_program).and_then(|bundles| {
        if bundles.is_empty() {
            None
        } else {
            Some(bundles.remove(0))
        }
    });

    if captured.get(&magic_program).is_some_and(Vec::is_empty) {
        captured.remove(&magic_program);
    }

    bundle
}

fn process_magic_program_mock(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data == EXECUTE_CAPTURED_STANDALONE_ACTION {
        return execute_captured_standalone_action(program_id, accounts);
    }

    let magic_ix: MagicBlockInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match magic_ix {
        MagicBlockInstruction::ScheduleTask(args) => {
            captured_schedules()
                .lock()
                .unwrap()
                .entry(*program_id)
                .or_default()
                .push(args);
        }
        MagicBlockInstruction::ScheduleIntentBundle(args) => {
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

fn execute_captured_standalone_action(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let bundle =
        pop_captured_intent_bundle(*program_id).ok_or(ProgramError::InvalidInstructionData)?;
    if bundle.args.standalone_actions.len() != 1 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let action = &bundle.args.standalone_actions[0];
    let escrow_authority = *bundle
        .schedule_accounts
        .get(action.escrow_authority as usize)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let escrow_index_seed = [action.args.escrow_index];
    let (escrow_signer, escrow_bump) = Pubkey::find_program_address(
        &[b"balance", escrow_authority.as_ref(), &escrow_index_seed],
        &ephemeral_rollups_pinocchio::ID,
    );

    let mut instruction_accounts: Vec<AccountMeta> = action
        .accounts
        .iter()
        .map(|meta| {
            if meta.is_writable {
                AccountMeta::new(convert_magic_pubkey(meta.pubkey), false)
            } else {
                AccountMeta::new_readonly(convert_magic_pubkey(meta.pubkey), false)
            }
        })
        .collect();
    instruction_accounts.push(AccountMeta::new_readonly(escrow_authority, false));
    instruction_accounts.push(AccountMeta::new_readonly(escrow_signer, true));

    let ix = Instruction {
        program_id: convert_magic_pubkey(action.destination_program),
        accounts: instruction_accounts,
        data: action.args.data.clone(),
    };

    let program_account = accounts
        .iter()
        .find(|account| account.key == &ix.program_id)
        .ok_or(ProgramError::NotEnoughAccountKeys)?
        .clone();
    let mut account_infos = Vec::with_capacity(ix.accounts.len() + 1);
    account_infos.push(program_account);
    for meta in &ix.accounts {
        let account_info = accounts
            .iter()
            .find(|account| account.key == &meta.pubkey)
            .ok_or(ProgramError::NotEnoughAccountKeys)?;
        account_infos.push(account_info.clone());
    }

    let escrow_bump_seed = [escrow_bump];
    let signer_seeds: &[&[u8]] = &[
        b"balance",
        escrow_authority.as_ref(),
        &escrow_index_seed,
        &escrow_bump_seed,
    ];

    invoke_signed(&ix, &account_infos, &[signer_seeds])
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

struct Fixture {
    context: ProgramTestContext,
    payer: Pubkey,
    mint: Pubkey,
    magic_program: Pubkey,
    task_context: Pubkey,
    queue: Pubkey,
    vault: Pubkey,
    vault_ata: Pubkey,
    source_ata: Pubkey,
    destination_ata: Pubkey,
    escrow_signer: Pubkey,
}

async fn latest_blockhash(context: &mut ProgramTestContext) -> solana_program::hash::Hash {
    context.banks_client.get_latest_blockhash().await.unwrap()
}

async fn setup_fixture() -> Fixture {
    let magic_program = solana_pubkey::Pubkey::new_from_array(
        ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes(),
    );
    let task_context = Pubkey::new_unique();
    clear_captured_schedules(magic_program);
    clear_captured_intent_bundles(magic_program);

    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    add_magic_program_mock(&mut pt, magic_program);
    pt.add_account(
        task_context,
        SolanaAccount {
            lamports: 1_000_000,
            data: vec![0; 8],
            owner: magic_program,
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let (escrow_signer, _) = Pubkey::find_program_address(
        &[
            b"balance",
            payer.as_ref(),
            &[EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX],
        ],
        &ephemeral_rollups_pinocchio::ID,
    );
    context.set_account(
        &escrow_signer,
        &AccountSharedData::from(SolanaAccount {
            lamports: Rent::default().minimum_balance(0).max(1),
            data: vec![],
            owner: ephemeral_rollups_pinocchio::ID,
            executable: false,
            rent_epoch: 0,
        }),
    );

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let pdas = utils::derive_pdas(PROGRAM, payer, mint);
    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        2,
    )
    .await;

    let queue = Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref()], &PROGRAM).0;
    let vault = pdas.vault;
    let vault_bump = pdas.bump_vault;
    let source_ata = setup.user_tokens[0];
    let destination_ata = setup.user_tokens[1];
    let (vault_eata, _vault_eata_bump) =
        Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
    let vault_ata = utils::derive_associated_token_address(vault, mint);

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
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, vault_bump],
    };

    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_vault, ix_init_queue],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    Fixture {
        context,
        payer,
        mint,
        magic_program,
        task_context,
        queue,
        vault,
        vault_ata,
        source_ata,
        destination_ata,
        escrow_signer,
    }
}

async fn enqueue_transfer(fixture: &mut Fixture, delay_seconds: u64) {
    let mut data = vec![instruction::DEPOSIT_AND_QUEUE_TRANSFER];
    data.extend_from_slice(&QUEUED_AMOUNT.to_le_bytes());
    data.extend_from_slice(&delay_seconds.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new_readonly(fixture.vault, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new(fixture.source_ata, false),
            AccountMeta::new(fixture.vault_ata, false),
            AccountMeta::new_readonly(fixture.destination_ata, false),
            AccountMeta::new_readonly(fixture.payer, true),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data,
    };

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
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
            AccountMeta::new(fixture.task_context, false),
            AccountMeta::new_readonly(magic_program, false),
        ],
        data: vec![instruction::ENSURE_TRANSFER_QUEUE_CRANK],
    }
}

fn process_queue_tick_ix(fixture: &Fixture) -> Instruction {
    Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(fixture.payer, false),
            AccountMeta::new(fixture.queue, false),
            AccountMeta::new(fixture.task_context, false),
            AccountMeta::new_readonly(fixture.magic_program, false),
        ],
        data: vec![internal::PROCESS_TRANSFER_QUEUE_TICK],
    }
}

fn execute_captured_standalone_action_ix(fixture: &Fixture) -> Instruction {
    Instruction {
        program_id: fixture.magic_program,
        accounts: vec![
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new_readonly(fixture.payer, false),
            AccountMeta::new_readonly(fixture.vault, false),
            AccountMeta::new_readonly(fixture.mint, false),
            AccountMeta::new(fixture.vault_ata, false),
            AccountMeta::new(fixture.destination_ata, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(fixture.escrow_signer, false),
        ],
        data: EXECUTE_CAPTURED_STANDALONE_ACTION.to_vec(),
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
    let _test_guard = test_lock().lock().await;
    let mut fixture = setup_fixture().await;

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
    assert_eq!(captured[0].execution_interval_millis, 400);
    assert_eq!(captured[0].iterations, i64::MAX);
    assert_eq!(captured[0].instructions.len(), 1);
    assert_eq!(
        captured[0].instructions[0].program_id.to_bytes(),
        PROGRAM.to_bytes()
    );
    assert_eq!(captured[0].instructions[0].accounts.len(), 4);
    assert_eq!(
        captured[0].instructions[0].accounts[0].pubkey.to_bytes(),
        fixture.payer.to_bytes()
    );
    assert_eq!(
        captured[0].instructions[0].accounts[1].pubkey.to_bytes(),
        fixture.queue.to_bytes()
    );
    assert_eq!(
        captured[0].instructions[0].accounts[2].pubkey.to_bytes(),
        fixture.task_context.to_bytes()
    );
    assert_eq!(
        captured[0].instructions[0].accounts[3].pubkey.to_bytes(),
        fixture.magic_program.to_bytes()
    );
    assert_eq!(
        captured[0].instructions[0].data,
        vec![internal::PROCESS_TRANSFER_QUEUE_TICK]
    );

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
    assert!(take_captured_schedules(fixture.magic_program).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn ensure_transfer_queue_crank_rejects_non_magic_program() {
    let _test_guard = test_lock().lock().await;
    let fake_magic_program = Pubkey::new_unique();

    let mut fixture = {
        let magic_program = solana_pubkey::Pubkey::new_from_array(
            ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID.to_bytes(),
        );
        let task_context = Pubkey::new_unique();
        clear_captured_schedules(magic_program);
        clear_captured_intent_bundles(magic_program);

        let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
        utils::add_associated_token_program(&mut pt);
        add_magic_program_mock(&mut pt, magic_program);
        add_noop_program_mock(&mut pt, fake_magic_program);
        pt.add_account(
            task_context,
            SolanaAccount {
                lamports: 1_000_000,
                data: vec![0; 8],
                owner: magic_program,
                executable: false,
                rent_epoch: 0,
            },
        );

        let mut context = pt.start_with_context().await;
        let payer = context.payer.pubkey();
        let (escrow_signer, _) = Pubkey::find_program_address(
            &[
                b"balance",
                payer.as_ref(),
                &[EXECUTE_READY_QUEUED_TRANSFER_ESCROW_INDEX],
            ],
            &ephemeral_rollups_pinocchio::ID,
        );
        context.set_account(
            &escrow_signer,
            &AccountSharedData::from(SolanaAccount {
                lamports: Rent::default().minimum_balance(0).max(1),
                data: vec![],
                owner: ephemeral_rollups_pinocchio::ID,
                executable: false,
                rent_epoch: 0,
            }),
        );

        let mint_kp = Keypair::new();
        let mint = mint_kp.pubkey();

        let pdas = utils::derive_pdas(PROGRAM, payer, mint);
        let setup = utils::setup_mint_and_token_accounts(
            &mut context,
            payer,
            &mint_kp,
            DECIMALS,
            STARTING_BALANCE,
            2,
        )
        .await;

        let queue = Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref()], &PROGRAM).0;
        let vault = pdas.vault;
        let vault_bump = pdas.bump_vault;
        let source_ata = setup.user_tokens[0];
        let destination_ata = setup.user_tokens[1];
        let (vault_eata, _vault_eata_bump) =
            Pubkey::find_program_address(&[vault.as_ref(), mint.as_ref()], &PROGRAM);
        let vault_ata = utils::derive_associated_token_address(vault, mint);

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
            data: vec![instruction::INITIALIZE_GLOBAL_VAULT, vault_bump],
        };

        let ix_init_queue = Instruction {
            program_id: PROGRAM,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(queue, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            ],
            data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
        };

        let tx_init = Transaction::new_signed_with_payer(
            &[ix_init_vault, ix_init_queue],
            Some(&payer),
            &[&context.payer],
            context.last_blockhash,
        );
        context
            .banks_client
            .process_transaction(tx_init)
            .await
            .unwrap();

        Fixture {
            context,
            payer,
            mint,
            magic_program,
            task_context,
            queue,
            vault,
            vault_ata,
            source_ata,
            destination_ata,
            escrow_signer,
        }
    };

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[ensure_queue_crank_ix_with_magic_program(
            &fixture,
            fake_magic_program,
        )],
        Some(&fixture.payer),
        &[&fixture.context.payer],
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

    let queue_after = queue_account(&mut fixture.context, fixture.queue).await;
    let header = read_header_unaligned(&queue_after.data);
    assert_eq!(header.crank_task_id, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn process_transfer_queue_tick_is_noop_when_queue_is_empty() {
    let _test_guard = test_lock().lock().await;
    let mut fixture = setup_fixture().await;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
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
async fn process_transfer_queue_tick_is_noop_when_next_transfer_is_not_ready() {
    let _test_guard = test_lock().lock().await;
    let mut fixture = setup_fixture().await;
    enqueue_transfer(&mut fixture, 120).await;

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
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
    let _test_guard = test_lock().lock().await;
    let mut fixture = setup_fixture().await;
    enqueue_transfer(&mut fixture, 0).await;

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
        program_id: convert_magic_pubkey(captured[0].instructions[0].program_id),
        accounts: captured[0].instructions[0]
            .accounts
            .iter()
            .map(|meta| AccountMeta {
                pubkey: convert_magic_pubkey(meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: captured[0].instructions[0].data.clone(),
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

    let queue_after_scheduling = queue_account(&mut fixture.context, fixture.queue).await;
    let header_after_scheduling = read_header_unaligned(&queue_after_scheduling.data);
    assert_eq!(header_after_scheduling.length, 0);

    let standalone_action = &captured_bundles[0].args.standalone_actions[0];
    assert_eq!(
        standalone_action.destination_program.to_bytes(),
        PROGRAM.to_bytes()
    );
    assert_eq!(standalone_action.compute_units, 100_000);
    assert_eq!(
        captured_bundles[0].schedule_accounts[standalone_action.escrow_authority as usize],
        fixture.payer
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
    assert_eq!(standalone_action.args.data, expected_action_data);
    assert_eq!(standalone_action.accounts.len(), 6);
    assert_eq!(
        standalone_action.accounts[0].pubkey.to_bytes(),
        fixture.payer.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[1].pubkey.to_bytes(),
        fixture.vault.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[2].pubkey.to_bytes(),
        fixture.mint.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[3].pubkey.to_bytes(),
        fixture.vault_ata.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[4].pubkey.to_bytes(),
        fixture.destination_ata.to_bytes()
    );
    assert_eq!(
        standalone_action.accounts[5].pubkey.to_bytes(),
        spl_token_interface::ID.to_bytes()
    );

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[process_queue_tick_ix(&fixture)],
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
    assert_eq!(peek_captured_intent_bundles(fixture.magic_program).len(), 1);

    let blockhash = latest_blockhash(&mut fixture.context).await;
    let tx = Transaction::new_signed_with_payer(
        &[execute_captured_standalone_action_ix(&fixture)],
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
    assert!(peek_captured_intent_bundles(fixture.magic_program).is_empty());

    let queue_after = queue_account(&mut fixture.context, fixture.queue).await;
    let header_after = read_header_unaligned(&queue_after.data);
    assert_eq!(header_after.length, 0);
    assert_eq!(
        token_amount(&mut fixture.context, fixture.source_ata).await,
        STARTING_BALANCE - QUEUED_AMOUNT
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.vault_ata).await,
        0
    );
    assert_eq!(
        token_amount(&mut fixture.context, fixture.destination_ata).await,
        QUEUED_AMOUNT
    );
}
