use dlp_api::state::DelegationRecord;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::fees_pda::{FeesPda, FEES_PDA_SEED, FEES_PDA_TAG};
use ephemeral_spl_api::state::{Initializable, RawType};
use ephemeral_spl_api::ID as PROGRAM;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError,
};
use solana_program_test::{processor, tokio};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod common;
mod utils;

const MAGIC_PROGRAM: Pubkey = Pubkey::new_from_array([7; 32]);

fn read_fees_pda(data: &[u8]) -> FeesPda {
    assert_eq!(data.len(), FeesPda::LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const FeesPda) }
}

fn process_magic_program_mock(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let [payer, magic_context, fees_pda, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if instruction_data != [1, 0, 0, 0] {
        return Err(ProgramError::InvalidInstructionData);
    }
    if !payer.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !fees_pda.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }

    magic_context.try_borrow_mut_data()?[0] = 1;
    Ok(())
}

#[tokio::test]
async fn initialize_fees_pda_is_idempotent() {
    let context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let validator = utils::test_pubkey("initialize_fees_pda_is_idempotent::validator");
    let (fees_pda, bump) =
        Pubkey::find_program_address(&[FEES_PDA_SEED, validator.as_ref()], &PROGRAM);

    let ix = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(fees_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_FEES_PDA],
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx,
        "fees_pda::init",
    )
    .await
    .unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_reinit =
        Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], second_blockhash);
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_reinit,
        "fees_pda::reinit",
    )
    .await
    .unwrap();

    let fees_pda_account = context
        .banks_client
        .get_account(fees_pda)
        .await
        .unwrap()
        .expect("fees pda must exist");

    assert_eq!(fees_pda_account.owner, PROGRAM);
    assert_eq!(fees_pda_account.data.len(), FeesPda::LEN);

    let fees_pda_state = read_fees_pda(&fees_pda_account.data);
    assert!(fees_pda_state.is_initialized());
    assert_eq!(fees_pda_state.tag, FEES_PDA_TAG);
    assert_eq!(fees_pda_state.validator, validator.to_bytes().into());
    assert_eq!(fees_pda_state.bump, bump);
}

#[tokio::test]
async fn delegate_fees_pda_is_idempotent() {
    let context = utils::start_program_test(PROGRAM).await;
    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let validator = utils::test_pubkey("delegate_fees_pda_is_idempotent::validator");
    let (fees_pda, bump) =
        Pubkey::find_program_address(&[FEES_PDA_SEED, validator.as_ref()], &PROGRAM);

    let ix_init = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(fees_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_FEES_PDA],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let (buffer_pda, _) = Pubkey::find_program_address(&[b"buffer", fees_pda.as_ref()], &PROGRAM);
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", fees_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", fees_pda.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );

    let ix_delegate = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(fees_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::DELEGATE_FEES_PDA],
    };

    let tx_delegate = Transaction::new_signed_with_payer(
        &[ix_delegate.clone()],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_delegate,
        "fees_pda::delegate",
    )
    .await
    .unwrap();

    let redelegate_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_redelegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&payer_kp],
        redelegate_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_redelegate,
        "fees_pda::redelegate",
    )
    .await
    .unwrap();

    let fees_pda_account = context
        .banks_client
        .get_account(fees_pda)
        .await
        .unwrap()
        .expect("delegated fees pda must exist");
    assert_eq!(
        fees_pda_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let fees_pda_state = read_fees_pda(&fees_pda_account.data);
    assert!(fees_pda_state.is_initialized());
    assert_eq!(fees_pda_state.tag, FEES_PDA_TAG);
    assert_eq!(fees_pda_state.validator, validator.to_bytes().into());
    assert_eq!(fees_pda_state.bump, bump);

    let delegation_record = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    let delegation_record =
        DelegationRecord::try_from_bytes_with_discriminator(&delegation_record.data)
            .expect("delegation record must deserialize");
    assert_eq!(delegation_record.authority.to_bytes(), validator.to_bytes());
}

#[tokio::test]
async fn commit_fees_pda_schedules_commit() {
    let mut context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.prefer_bpf(false);
        pt.add_program(
            "magic_program_mock",
            MAGIC_PROGRAM,
            processor!(process_magic_program_mock),
        );
        pt.prefer_bpf(true);
    })
    .await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let validator = utils::test_pubkey("commit_fees_pda_schedules_commit::validator");
    let (fees_pda, _) =
        Pubkey::find_program_address(&[FEES_PDA_SEED, validator.as_ref()], &PROGRAM);
    let magic_context = utils::test_pubkey("commit_fees_pda_schedules_commit::magic_context");

    let ix_init = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(fees_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_FEES_PDA],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init],
        Some(&payer),
        &[&payer_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    context.set_account(
        &magic_context,
        &Account {
            lamports: 1,
            data: vec![0],
            owner: MAGIC_PROGRAM,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );

    let commit_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let ix_commit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(fees_pda, false),
            AccountMeta::new_readonly(validator, false),
            AccountMeta::new(magic_context, false),
            AccountMeta::new_readonly(MAGIC_PROGRAM, false),
        ],
        data: vec![instruction::COMMIT_FEES_PDA],
    };

    let tx_commit = Transaction::new_signed_with_payer(
        &[ix_commit],
        Some(&payer),
        &[&payer_kp],
        commit_blockhash,
    );
    common::metrics::process_transaction_record_cu(
        &context.banks_client,
        tx_commit,
        "fees_pda::commit",
    )
    .await
    .unwrap();

    let magic_context_account = context
        .banks_client
        .get_account(magic_context)
        .await
        .unwrap()
        .expect("magic context must exist");
    assert_eq!(magic_context_account.data, vec![1]);
}
