use solana_keypair::Keypair;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    rent::Rent,
    sysvar::Sysvar,
};
use solana_program_pack::Pack;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;
use spl_token_interface::instruction::{initialize_account, initialize_account3, initialize_mint};
use spl_token_interface::state::{Account as SplAccount, Mint};

#[allow(dead_code)]
pub struct Pdas {
    pub ephemeral_ata: Pubkey,
    pub vault: Pubkey,
}

#[allow(dead_code)]
pub struct TokenSetup {
    pub user_tokens: Vec<Pubkey>,
}

const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub fn associated_token_program_id() -> Pubkey {
    ASSOCIATED_TOKEN_PROGRAM_ID
}

fn process_associated_token_program_mock(
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

pub fn add_associated_token_program(pt: &mut ProgramTest) {
    pt.prefer_bpf(false);
    pt.add_program(
        "spl_associated_token_account",
        ASSOCIATED_TOKEN_PROGRAM_ID,
        processor!(process_associated_token_program_mock),
    );
    pt.prefer_bpf(true);
}

pub fn derive_associated_token_address(wallet: Pubkey, mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            wallet.as_ref(),
            spl_token_interface::ID.as_ref(),
            mint.as_ref(),
        ],
        &ASSOCIATED_TOKEN_PROGRAM_ID,
    )
    .0
}

#[allow(dead_code)]
pub fn derive_pdas(program: Pubkey, owner: Pubkey, mint: Pubkey) -> Pdas {
    let (ephemeral_ata, _) = Pubkey::find_program_address(
        &[owner.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &program,
    );
    let (vault, _) = Pubkey::find_program_address(&[mint.to_bytes().as_slice()], &program);
    Pdas {
        ephemeral_ata,
        vault,
    }
}

#[allow(dead_code)]
pub fn derive_shuttle_ephemeral_ata(
    program: Pubkey,
    owner: Pubkey,
    mint: Pubkey,
    shuttle_id: u32,
) -> (Pubkey, u8) {
    let shuttle_id_seed = shuttle_id.to_le_bytes();
    Pubkey::find_program_address(
        &[owner.as_ref(), mint.as_ref(), shuttle_id_seed.as_ref()],
        &program,
    )
}

#[allow(dead_code)]
pub fn derive_shuttle_eata(program: Pubkey, shuttle: Pubkey, mint: Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[shuttle.as_ref(), mint.as_ref()], &program)
}

// Creates and initializes:
// - Mint (with mint authority = payer, freeze authority = payer)
// - `user_accounts` token accounts owned by `payer`
// - mints `starting_balance` to the first user token account
// Submits a single transaction for all instructions.
pub async fn setup_mint_and_token_accounts(
    context: &mut ProgramTestContext,
    payer: Pubkey,
    mint_kp: &Keypair,
    decimals: u8,
    starting_balance: u64,
    user_accounts: usize,
) -> TokenSetup {
    assert!(
        user_accounts >= 1,
        "at least one user token account required"
    );

    let mint = mint_kp.pubkey();

    let rent = context.banks_client.get_rent().await.unwrap();

    let mint_space = Mint::LEN;
    let mint_lamports = rent.minimum_balance(mint_space);

    let mut instructions = vec![];
    let mut signers: Vec<&Keypair> = vec![&context.payer, mint_kp];

    // Create and init mint
    instructions.push(create_account(
        &payer,
        &mint,
        mint_lamports,
        mint_space as u64,
        &spl_token_interface::ID,
    ));

    let mut init_mint_ix = initialize_mint(
        &spl_token_interface::ID,
        &mint,
        &payer,
        Some(&payer),
        decimals,
    )
    .unwrap();
    init_mint_ix.program_id = spl_token_interface::ID;
    instructions.push(init_mint_ix);

    // Create user atas
    let token_acc_space = SplAccount::LEN;
    let token_acc_lamports = rent.minimum_balance(token_acc_space);

    let mut user_tokens: Vec<Pubkey> = vec![];
    let mut user_token_kps: Vec<Keypair> = vec![];

    for _ in 0..user_accounts {
        let kp = Keypair::new();
        let pk = kp.pubkey();
        user_token_kps.push(kp);
        user_tokens.push(pk);

        instructions.push(create_account(
            &payer,
            &pk,
            token_acc_lamports,
            token_acc_space as u64,
            &spl_token_interface::ID,
        ));

        let mut init_user_ix =
            initialize_account(&spl_token_interface::ID, &pk, &mint, &payer).unwrap();
        init_user_ix.program_id = spl_token_interface::ID;
        instructions.push(init_user_ix);
    }

    // Add user token signers
    for kp in &user_token_kps {
        signers.push(kp);
    }

    // Mint starting balance to first user token
    let first_user = user_tokens[0];
    let mut mint_to_ix = spl_token_interface::instruction::mint_to(
        &spl_token_interface::ID,
        &mint,
        &first_user,
        &payer,
        &[],
        starting_balance,
    )
    .unwrap();
    mint_to_ix.program_id = spl_token_interface::ID;
    instructions.push(mint_to_ix);

    // Submit transaction
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer),
        &signers,
        context.last_blockhash,
    );

    context.banks_client.process_transaction(tx).await.unwrap();

    TokenSetup { user_tokens }
}
