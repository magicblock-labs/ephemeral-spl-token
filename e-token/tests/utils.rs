//! Shared helpers for integration tests. Each `tests/*.rs` binary only uses a subset; suppress
//! `dead_code` for the rest.
#![allow(dead_code)]

use solana_account::Account;
use solana_keypair::Keypair;
use solana_program::{
    account_info::AccountInfo,
    bpf_loader,
    entrypoint::ProgramResult,
    hash::hash,
    native_token::LAMPORTS_PER_SOL,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    rent::Rent,
    sysvar::Sysvar,
};
use solana_program_pack::Pack;
use solana_program_test::{processor, read_file, ProgramTest, ProgramTestContext};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;
use spl_token_interface::instruction::{initialize_account, initialize_account3, initialize_mint};
use spl_token_interface::state::{Account as SplAccount, Mint};

// this must be same as ESplInternalInstruction
#[repr(u8)]
pub enum TestInternalInstruction {
    UndelegationCallback = 196,

    SettleAndCloseShuttleIntent = 201,
    ExecuteReadyQueuedTransfer = 202,
    ProcessTransferQueueTick = 203,
    TransferLamportsPda = 204,
    UndelegateLamportsPda = 205,
    CloseLamportsPdaIntent = 206,
    MarkTransferQueueRefillPending = 207,
    ExecuteTransferCallback = 208,
    InitializeGroupReceipt = 209,
}

impl TestInternalInstruction {
    pub const fn discriminator(self) -> u8 {
        self as u8
    }
    pub const fn to_bytes(self) -> [u8; 1] {
        [self.discriminator()]
    }
    pub fn to_vec(self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }
    pub fn with_data(self, instruction_data: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(1 + instruction_data.len());
        data.extend_from_slice(&self.to_bytes());
        data.extend_from_slice(instruction_data);
        data
    }
}

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

/// Deterministic pubkey for integration tests (SHA-256 over `e-token::pubkey::{label}`).
pub fn test_pubkey(label: &str) -> Pubkey {
    let input = format!("e-token::pubkey::{label}");
    Pubkey::new_from_array(hash(input.as_bytes()).to_bytes())
}

/// Deterministic signing keypair for integration tests (SHA-256 over `e-token::keypair::{label}`).
pub fn test_keypair(label: &str) -> Keypair {
    let input = format!("e-token::keypair::{label}");
    Keypair::new_from_array(hash(input.as_bytes()).to_bytes())
}

/// Label for [`fixed_payer_keypair`] (stable across runs for reproducible CU metrics).
pub const FIXED_PAYER_LABEL: &str = "e_token::fixed_payer";

/// Genesis-funded payer shared by integration tests. Prefer this over [`ProgramTestContext::payer`]
/// for signing and for pubkeys used in PDA seeds so compute-unit measurements match across runs.
pub fn fixed_payer_keypair() -> Keypair {
    test_keypair(FIXED_PAYER_LABEL)
}

/// Fund [`fixed_payer_keypair`] at genesis (same order of lamports as the default mint in
/// `solana-program-test`). Call after [`ProgramTest::new`] and before [`ProgramTest::start_with_context`].
pub fn fund_fixed_payer_genesis(pt: &mut ProgramTest) {
    let payer = fixed_payer_keypair();
    pt.add_genesis_account(
        payer.pubkey(),
        Account {
            lamports: 1_000_000 * LAMPORTS_PER_SOL,
            data: vec![],
            owner: solana_system_interface::program::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
}

/// Permission program id (matches `tests/fixtures/acl.so`).
pub fn permission_program_id() -> Pubkey {
    let bytes: [u8; 32] = ephemeral_rollups_pinocchio::acl::consts::PERMISSION_PROGRAM_ID
        .as_ref()
        .try_into()
        .expect("PERMISSION_PROGRAM_ID must be 32 bytes");
    Pubkey::new_from_array(bytes)
}

/// Load a BPF `.so` from `tests/fixtures` (or `BPF_OUT_DIR`) as an executable account.
pub fn add_executable_bpf_fixture(
    pt: &mut ProgramTest,
    program_address: Pubkey,
    fixture_path: &str,
) {
    let data = read_file(fixture_path);
    let rent = Rent::default();
    pt.add_account(
        program_address,
        Account {
            lamports: rent.minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );
}

/// Prefer BPF, fund [`fixed_payer_keypair`] at genesis, add the SPL ATA mock, and load `acl.so` / `dlp.so`.
pub fn configure_standard_program_test(pt: &mut ProgramTest) {
    pt.prefer_bpf(true);
    fund_fixed_payer_genesis(pt);
    add_associated_token_program(pt);
    add_executable_bpf_fixture(pt, permission_program_id(), "tests/fixtures/acl.so");
    add_executable_bpf_fixture(pt, ephemeral_rollups_pinocchio::ID, "tests/fixtures/dlp.so");
}

/// Same as [`start_program_test_with`] with no extra setup.
pub async fn start_program_test(program_id: Pubkey) -> ProgramTestContext {
    start_program_test_with(program_id, |_| {}).await
}

/// Build [`ProgramTest`], apply [`configure_standard_program_test`], then `configure`, then start.
///
/// Sign with [`fixed_payer_keypair`] and use its pubkey for fee payer / seeds. The harness
/// `context.payer` remains random and should not be used.
pub async fn start_program_test_with<F: FnOnce(&mut ProgramTest)>(
    program_id: Pubkey,
    configure: F,
) -> ProgramTestContext {
    // `ProgramTest::new` calls `add_program` before any `prefer_bpf` override; default `prefer_bpf`
    // is false unless BPF_OUT_DIR is set, so the main program would hit "processor not available".
    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.add_program("ephemeral_token_program", program_id, None);
    configure_standard_program_test(&mut pt);
    configure(&mut pt);
    pt.start_with_context().await
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

pub fn add_permission_program(pt: &mut ProgramTest) {
    let data = read_file("tests/fixtures/acl.so");
    pt.add_account(
        permission_program_id(),
        solana_account::Account {
            lamports: Rent::default().minimum_balance(data.len()).max(1),
            data,
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 0,
        },
    );
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
    payer_signer: &Keypair,
    mint_kp: &Keypair,
    decimals: u8,
    starting_balance: u64,
    user_accounts: usize,
) -> TokenSetup {
    assert!(
        user_accounts >= 1,
        "at least one user token account required"
    );

    let payer = payer_signer.pubkey();
    let mint = mint_kp.pubkey();

    let rent = context.banks_client.get_rent().await.unwrap();

    let mint_space = Mint::LEN;
    let mint_lamports = rent.minimum_balance(mint_space);

    let mut instructions = vec![];
    let mut signers: Vec<&Keypair> = vec![payer_signer, mint_kp];

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

    for i in 0..user_accounts {
        let kp = test_keypair(&format!(
            "setup_mint_and_token_accounts::user_token_{i}_{}",
            mint_kp.pubkey()
        ));
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
