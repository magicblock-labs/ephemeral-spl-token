use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_program_test::ProgramTestContext;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;
use spl_token_interface::instruction::{initialize_account, initialize_mint};
use spl_token_interface::state::{Account as SplAccount, Mint};

pub struct Pdas {
    pub ephemeral_ata: Pubkey,
    pub bump_ata: u8,
    pub vault: Pubkey,
    pub bump_vault: u8,
}

#[allow(dead_code)]
pub struct TokenSetup {
    pub user_tokens: Vec<Pubkey>,
    pub vault_token: Pubkey,
}

pub fn derive_pdas(program: Pubkey, owner: Pubkey, mint: Pubkey) -> Pdas {
    let (ephemeral_ata, bump_ata) = Pubkey::find_program_address(
        &[owner.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &program,
    );
    let (vault, bump_vault) = Pubkey::find_program_address(&[mint.to_bytes().as_slice()], &program);
    Pdas {
        ephemeral_ata,
        bump_ata,
        vault,
        bump_vault,
    }
}

// Creates and initializes:
// - Mint (with mint authority = payer, freeze authority = payer)
// - `user_accounts` token accounts owned by `payer`
// - one vault token account owned by `vault_owner`
// - mints `starting_balance` to the first user token account
// Submits a single transaction for all instructions.
pub async fn setup_mint_and_token_accounts(
    context: &mut ProgramTestContext,
    payer: Pubkey,
    mint_kp: &Keypair,
    vault_owner: Pubkey,
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

    // Create vault ata
    let vault_token_kp = Keypair::new();
    let vault_token = vault_token_kp.pubkey();
    instructions.push(create_account(
        &payer,
        &vault_token,
        token_acc_lamports,
        token_acc_space as u64,
        &spl_token_interface::ID,
    ));
    let mut init_vault_ix =
        initialize_account(&spl_token_interface::ID, &vault_token, &mint, &vault_owner).unwrap();
    init_vault_ix.program_id = spl_token_interface::ID;
    instructions.push(init_vault_ix);
    signers.push(&vault_token_kp);

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

    TokenSetup {
        user_tokens,
        vault_token,
    }
}
