use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use magicblock_magic_program_api::instruction::CallbackInstruction;
use magicblock_magic_program_api::pda::{CALLBACK_SEED, CALLBACK_SIGNER_BUMP, CRANK_SEED, CRANK_SIGNER_BUMP};
use magicblock_magic_program_api::CALLBACK_PROGRAM_ID;
use pinocchio::error::ProgramError;
use pinocchio::ProgramResult;
use solana_instruction::Instruction;
use solana_program::account_info::AccountInfo;
use solana_program::program::invoke_signed;
use solana_program::pubkey::Pubkey;
use solana_program_test::ProgramTest;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn captured_execute_callbacks() -> &'static Mutex<Vec<Instruction>> {
    static S: OnceLock<Mutex<Vec<Instruction>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn take_execute_callbacks() -> Vec<Instruction> {
    std::mem::take(captured_execute_callbacks().lock().unwrap().as_mut())
}

pub fn process(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let ix: CallbackInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match ix {
        CallbackInstruction::ExecuteCallback { instruction } => {
            let map = accounts
                .iter()
                .map(|account| (*account.key, account))
                .collect::<HashMap<_, _>>();

            let account_infos = instruction
                .accounts
                .iter()
                .map(|meta| map[&meta.pubkey].clone())
                .collect::<Vec<_>>();

            invoke_signed(
                &instruction,
                &account_infos,
                &[&[CALLBACK_SEED, &[CALLBACK_SIGNER_BUMP]]],
            )?;

            captured_execute_callbacks()
                .lock()
                .unwrap()
                .push(instruction.clone());

            Ok(())
        }
    }
}

pub fn add_mock(pt: &mut ProgramTest) {
    use solana_program_test::processor;
    pt.prefer_bpf(false);
    pt.add_program("callback_mock", CALLBACK_PROGRAM_ID, processor!(process));
    pt.prefer_bpf(true);
}
