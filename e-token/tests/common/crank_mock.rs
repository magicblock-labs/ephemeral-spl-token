use magicblock_magic_program_api::instruction::MagicBlockInstruction;
use magicblock_magic_program_api::pda::{CRANK_SEED, CRANK_SIGNER_BUMP};
use magicblock_magic_program_api::CRANK_PROGRAM_ID;
use pinocchio::error::ProgramError;
use pinocchio::ProgramResult;
use solana_instruction::Instruction;
use solana_program::account_info::AccountInfo;
use solana_program::program::invoke_signed;
use solana_program::pubkey::Pubkey;
use solana_program_test::ProgramTest;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn captured_execute_cranks() -> &'static Mutex<Vec<Vec<Instruction>>> {
    static S: OnceLock<Mutex<Vec<Vec<Instruction>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn take_execute_cranks() -> Vec<Vec<Instruction>> {
    std::mem::take(captured_execute_cranks().lock().unwrap().as_mut())
}

pub fn process(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let ix: MagicBlockInstruction =
        bincode::deserialize(instruction_data).map_err(|_| ProgramError::InvalidInstructionData)?;

    match ix {
        MagicBlockInstruction::ExecuteCrank { instructions } => {
            let map = accounts
                .iter()
                .map(|account| (*account.key, account))
                .collect::<HashMap<_, _>>();

            for instruction in &instructions {
                let account_infos = instruction
                    .accounts
                    .iter()
                    .map(|meta| map[&meta.pubkey].clone())
                    .collect::<Vec<_>>();

                invoke_signed(
                    instruction,
                    &account_infos,
                    &[&[CRANK_SEED, &[CRANK_SIGNER_BUMP]]],
                )?;
            }

            captured_execute_cranks().lock().unwrap().push(instructions);

            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

pub fn add_mock(pt: &mut ProgramTest) {
    use solana_program_test::processor;
    pt.prefer_bpf(false);
    pt.add_program("crank_mock", CRANK_PROGRAM_ID, processor!(process));
    pt.prefer_bpf(true);
}
