use dlp_api::state::DelegationRecord;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::fees_pda::{FeesPda, FEES_PDA_SEED, FEES_PDA_TAG};
use ephemeral_spl_api::state::{Initializable, RawType};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError, rent::Rent,
};
use solana_program_test::{processor, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
const MAGIC_PROGRAM: Pubkey = Pubkey::new_from_array([7; 32]);

fn read_fees_pda(data: &[u8]) -> FeesPda {
    assert_eq!(data.len(), FeesPda::LEN);
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const FeesPda) }
}

fn parse_delegate_validator(data: &[u8]) -> Result<(u32, Option<[u8; 32]>), ProgramError> {
    if data.len() < 8 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let commit_frequency_ms = u32::from_le_bytes(
        data[0..4]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let seeds_len = u32::from_le_bytes(
        data[4..8]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    ) as usize;

    let mut offset = 8;
    for _ in 0..seeds_len {
        let seed_len = u32::from_le_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ) as usize;
        offset += 4 + seed_len;
        if offset > data.len() {
            return Err(ProgramError::InvalidInstructionData);
        }
    }

    let has_validator = *data
        .get(offset)
        .ok_or(ProgramError::InvalidInstructionData)?;
    offset += 1;

    let validator = match has_validator {
        0 => None,
        1 => {
            let mut validator = [0_u8; 32];
            validator.copy_from_slice(
                data.get(offset..offset + 32)
                    .ok_or(ProgramError::InvalidInstructionData)?,
            );
            Some(validator)
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    };

    Ok((commit_frequency_ms, validator))
}

fn process_delegation_program_mock(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let [_payer, delegated_account, owner_program, buffer, delegation_record, _delegation_metadata, system_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if instruction_data.len() < 8 || instruction_data[0] != 19 {
        return Err(ProgramError::InvalidInstructionData);
    }
    if *system_program.key != solana_system_interface::program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (commit_frequency_ms, validator) = parse_delegate_validator(&instruction_data[8..])?;
    let validator = validator.ok_or(ProgramError::InvalidInstructionData)?;

    {
        let buffer_data = buffer.try_borrow_data()?;
        let mut delegated_data = delegated_account.try_borrow_mut_data()?;
        if delegated_data.len() != buffer_data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        delegated_data.copy_from_slice(&buffer_data);
    }

    let record = DelegationRecord {
        authority: validator.into(),
        owner: owner_program.key.to_bytes().into(),
        delegation_slot: 0,
        lamports: delegated_account.lamports(),
        commit_frequency_ms: commit_frequency_ms as u64,
    };
    record
        .to_bytes_with_discriminator(&mut delegation_record.try_borrow_mut_data()?)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    Ok(())
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
    let pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    let context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let validator = Pubkey::new_unique();
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
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let second_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_reinit = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&context.payer],
        second_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_reinit)
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
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(false);
    pt.add_program(
        "delegation_program_mock",
        ephemeral_rollups_pinocchio::ID,
        processor!(process_delegation_program_mock),
    );
    pt.prefer_bpf(true);

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let validator = Pubkey::new_unique();
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
        &[&context.payer],
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

    context.set_account(
        &delegation_record_pda,
        &Account {
            lamports: Rent::default()
                .minimum_balance(DelegationRecord::size_with_discriminator())
                .max(1),
            data: vec![0; DelegationRecord::size_with_discriminator()],
            owner: ephemeral_rollups_pinocchio::ID,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
    );
    context.set_account(
        &delegation_metadata_pda,
        &Account {
            lamports: 1,
            data: vec![],
            owner: ephemeral_rollups_pinocchio::ID,
            executable: false,
            rent_epoch: 0,
        }
        .into(),
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
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_delegate)
        .await
        .unwrap();

    let redelegate_blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let tx_redelegate = Transaction::new_signed_with_payer(
        &[ix_delegate],
        Some(&payer),
        &[&context.payer],
        redelegate_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_redelegate)
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
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(false);
    pt.add_program(
        "magic_program_mock",
        MAGIC_PROGRAM,
        processor!(process_magic_program_mock),
    );
    pt.prefer_bpf(true);

    let mut context = pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let validator = Pubkey::new_unique();
    let (fees_pda, _) =
        Pubkey::find_program_address(&[FEES_PDA_SEED, validator.as_ref()], &PROGRAM);
    let magic_context = Pubkey::new_unique();

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
        &[&context.payer],
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
        &[&context.payer],
        commit_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_commit)
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
