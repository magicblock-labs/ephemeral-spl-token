use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    rent::Rent,
    sysvar::Sysvar,
};
use solana_program_pack::Pack;
use solana_pubkey::{pubkey, Pubkey};
use solana_system_interface::instruction::create_account;
use spl_token_interface::instruction::initialize_account3;
use spl_token_interface::state::Account as SplAccount;

const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub fn associated_token_program_id() -> Pubkey {
    ASSOCIATED_TOKEN_PROGRAM_ID
}

pub fn process_associated_token_program_mock(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.is_empty() || instruction_data[0] > 1 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let [funding, ata, wallet, mint, system_program, token_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if *system_program.key != solana_system_interface::program::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if *token_program.key != spl_token_interface::ID {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (expected_ata, bump_seed) = Pubkey::find_program_address(
        &[
            wallet.key.as_ref(),
            token_program.key.as_ref(),
            mint.key.as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    );
    if expected_ata != *ata.key {
        return Err(ProgramError::InvalidSeeds);
    }

    // Idempotent path: account already exists as a token account.
    if *ata.owner == *token_program.key && ata.data_len() == SplAccount::LEN {
        let ata_data = ata.try_borrow_data()?;
        let ata_state = SplAccount::unpack(&ata_data)?;
        if ata_state.mint != *mint.key || ata_state.owner != *wallet.key {
            return Err(ProgramError::InvalidAccountData);
        }
        return Ok(());
    }

    let lamports = Rent::get()?.minimum_balance(SplAccount::LEN);
    let bump = [bump_seed];
    let ata_signer_seeds: &[&[u8]] = &[
        wallet.key.as_ref(),
        token_program.key.as_ref(),
        mint.key.as_ref(),
        &bump,
    ];

    invoke_signed(
        &create_account(
            funding.key,
            ata.key,
            lamports,
            SplAccount::LEN as u64,
            token_program.key,
        ),
        &[funding.clone(), ata.clone()],
        &[ata_signer_seeds],
    )?;

    let mut init_ix = initialize_account3(token_program.key, ata.key, mint.key, wallet.key)?;
    init_ix.program_id = *token_program.key;
    invoke(&init_ix, &[ata.clone(), mint.clone()])?;

    Ok(())
}
