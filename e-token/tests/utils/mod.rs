use ephemeral_rollups_pinocchio::acl::permission_pda_from_permissioned_account;
use ephemeral_spl_api::state::{
    shuttle_ephemeral_ata::ShuttleEphemeralAta, transfer_queue::TransferQueue,
};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_program_test::ProgramTestContext;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use solana_transaction::Transaction;
use spl_token_interface::instruction::{initialize_account, initialize_mint};
use spl_token_interface::state::{Account as SplAccount, Mint};

mod associated_token;
mod instructions;
mod setup;

pub use associated_token::*;
pub use instructions::*;
pub use setup::*;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pdas {
    pub ephemeral_ata: Pubkey,
    pub bump_ata: u8,
    pub vault: Pubkey,
    pub bump_vault: u8,
    pub queue: Pubkey,
    pub queue_bump: u8,
    pub queue_permission_pda: Pubkey,
    pub queue_ata: Pubkey,
    pub queue_eata: Pubkey,
    pub queue_eata_bump: u8,
    pub queue_eata_permission_pda: Pubkey,
    pub queue_shuttle: Pubkey,
    pub queue_shuttle_bump: u8,
    pub queue_shuttle_ata: Pubkey,
    pub queue_shuttle_eata: Pubkey,
    pub queue_shuttle_eata_bump: u8,
}

#[allow(dead_code)]
pub struct TokenSetup {
    pub user_tokens: Vec<Pubkey>,
    pub recipient_tokens: Vec<Pubkey>,
}

pub fn derive_associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            wallet.as_ref(),
            spl_token_interface::ID.as_ref(),
            mint.as_ref(),
        ],
        &associated_token_program_id(),
    )
    .0
}

#[allow(dead_code)]
pub fn derive_pdas(program: Pubkey, owner: Pubkey, mint: Pubkey, queue_shuttle_id: u32) -> Pdas {
    let (ephemeral_ata, bump_ata) = Pubkey::find_program_address(
        &[owner.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &program,
    );
    let (vault, bump_vault) = Pubkey::find_program_address(&[mint.to_bytes().as_slice()], &program);
    let (queue, queue_bump) = TransferQueue::find_pda(&mint);
    let queue_permission_pda = permission_pda_from_permissioned_account(&queue);
    let queue_ata = derive_associated_token_address(&queue, &mint);
    let (queue_eata, queue_eata_bump) = Pubkey::find_program_address(
        &[queue.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    let queue_eata_permission_pda = permission_pda_from_permissioned_account(&queue_eata);
    let (queue_shuttle, queue_shuttle_bump) =
        ShuttleEphemeralAta::find_pda(&queue, &mint, queue_shuttle_id);
    let queue_shuttle_ata = derive_associated_token_address(&queue_shuttle, &mint);
    let (queue_shuttle_eata, queue_shuttle_eata_bump) = Pubkey::find_program_address(
        &[queue_shuttle.as_ref(), mint.as_ref()],
        &ephemeral_spl_api::program::ID,
    );
    Pdas {
        ephemeral_ata,
        bump_ata,
        vault,
        bump_vault,
        queue,
        queue_bump,
        queue_permission_pda,
        queue_ata,
        queue_eata,
        queue_eata_bump,
        queue_eata_permission_pda,
        queue_shuttle,
        queue_shuttle_bump,
        queue_shuttle_ata,
        queue_shuttle_eata,
        queue_shuttle_eata_bump,
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

    // Create recipient accounts
    let mut recipient_tokens: Vec<Pubkey> = vec![];
    let mut recipient_token_kps: Vec<Keypair> = vec![];
    for _ in 0..user_accounts {
        let kp = Keypair::new();
        let pk = kp.pubkey();
        recipient_token_kps.push(kp);
        recipient_tokens.push(pk);

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
    for kp in user_token_kps.iter().chain(recipient_token_kps.iter()) {
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

    TokenSetup {
        user_tokens,
        recipient_tokens,
    }
}
