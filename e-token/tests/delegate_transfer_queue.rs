use dlp_api::state::DelegationRecord;
use ephemeral_rollups_pinocchio::acl::{
    permission_pda_from_permissioned_account, PERMISSION_PROGRAM_ID,
};
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::transfer_queue::{
    header_len, TransferQueueHeader, QUEUE_SEED, TRANSFER_QUEUE_VERSION,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError, rent::Rent,
};
use solana_program_test::{processor, tokio, ProgramTest};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

mod utils;

use crate::utils::add_permission_program;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);
pub const VALIDATOR: Pubkey = Pubkey::new_from_array([77; 32]);

fn read_header_unaligned(data: &[u8]) -> TransferQueueHeader {
    assert!(data.len() >= header_len());
    unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) }
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
        offset += 4;
        offset += seed_len;
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
    if validator != Some(VALIDATOR.to_bytes()) {
        return Err(ProgramError::InvalidInstructionData);
    }

    {
        let buffer_data = buffer.try_borrow_data()?;
        let mut delegated_data = delegated_account.try_borrow_mut_data()?;
        if delegated_data.len() != buffer_data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        delegated_data.copy_from_slice(&buffer_data);
    }

    let record = DelegationRecord {
        authority: solana_system_interface::program::ID.to_bytes().into(),
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

#[tokio::test]
async fn delegate_transfer_queue_succeeds_and_is_idempotent() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    pt.prefer_bpf(false);
    pt.add_program(
        "delegation_program_mock",
        ephemeral_rollups_pinocchio::ID,
        processor!(process_delegation_program_mock),
    );
    add_permission_program(&mut pt);
    pt.prefer_bpf(true);

    let mint = Pubkey::new_unique();
    pt.add_account(
        mint,
        Account {
            lamports: 1,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );

    let context = &mut pt.start_with_context().await;
    let payer = context.payer.pubkey();
    let (queue, bump) =
        Pubkey::find_program_address(&[QUEUE_SEED, mint.as_ref(), VALIDATOR.as_ref()], &PROGRAM);
    let queue_permission = permission_pda_from_permissioned_account(&queue);

    let ix_init_queue = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(queue, false),
            AccountMeta::new(queue_permission, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(VALIDATOR, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(PERMISSION_PROGRAM_ID, false),
        ],
        data: vec![instruction::INITIALIZE_TRANSFER_QUEUE],
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_queue],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let queue_permission_account = context
        .banks_client
        .get_account(queue_permission)
        .await
        .unwrap()
        .expect("queue permission account must exist");

    assert_eq!(queue_permission_account.owner, PERMISSION_PROGRAM_ID);

    let (buffer_pda, _) = Pubkey::find_program_address(&[b"buffer", queue.as_ref()], &PROGRAM);
    let (delegation_record_pda, _) = Pubkey::find_program_address(
        &[b"delegation", queue.as_ref()],
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
    );
    let (delegation_metadata_pda, _) = Pubkey::find_program_address(
        &[b"delegation-metadata", queue.as_ref()],
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
            AccountMeta::new(queue, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(buffer_pda, false),
            AccountMeta::new(delegation_record_pda, false),
            AccountMeta::new(delegation_metadata_pda, false),
            AccountMeta::new_readonly(ephemeral_rollups_pinocchio::ID, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::DELEGATE_TRANSFER_QUEUE],
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

    let queue_account = context
        .banks_client
        .get_account(queue)
        .await
        .unwrap()
        .expect("queue account must exist");

    assert_eq!(
        queue_account.owner,
        ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
    );

    let delegation_record = context
        .banks_client
        .get_account(delegation_record_pda)
        .await
        .unwrap()
        .expect("delegation record must exist");
    let delegation_record =
        DelegationRecord::try_from_bytes_with_discriminator(&delegation_record.data)
            .expect("delegation record must deserialize");

    let header = read_header_unaligned(&queue_account.data);
    assert_eq!(header.version, TRANSFER_QUEUE_VERSION);
    assert_eq!(header.bump, bump);
    assert_eq!(
        header.mint,
        ephemeral_spl_api::Address::new_from_array(mint.to_bytes())
    );
    assert_eq!(header.length, 0);
    assert_eq!(
        delegation_record.authority.to_bytes(),
        solana_system_interface::program::ID.to_bytes()
    );
}
