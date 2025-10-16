use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::instruction::{initialize_account, initialize_mint};
use spl_token_interface::state::{Account, Mint};
use {
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_system_interface::instruction::create_account,
    solana_transaction::Transaction,
};

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6; // canonical USDC decimals
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32); // payer holds 10,000 tokens

#[tokio::test]
async fn deposit_spl_tokens_increments_ephemeral_amount() {
    let context = ProgramTest::new("ephemeral_token_program", PROGRAM, None)
        .start_with_context()
        .await;

    let payer = context.payer.pubkey();

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    // Derive PDAs based on the mint
    let (ephemeral_ata, bump_ata) = Pubkey::find_program_address(
        &[payer.to_bytes().as_slice(), mint.to_bytes().as_slice()],
        &PROGRAM,
    );
    let (vault, bump_vault) = Pubkey::find_program_address(&[mint.to_bytes().as_slice()], &PROGRAM);

    // 0) Create and initialize the Mint account
    let rent = context.banks_client.get_rent().await.unwrap();
    let mint_space = Mint::LEN;
    let mint_lamports = rent.minimum_balance(mint_space as usize);

    let create_mint_ix = create_account(
        &payer,
        &mint,
        mint_lamports,
        mint_space as u64,
        &spl_token_interface::ID,
    );

    let mut init_mint_ix = initialize_mint(
        &spl_token_interface::ID,
        &mint,
        &payer,       // mint authority
        Some(&payer), // freeze authority
        DECIMALS,
    )
    .unwrap();
    // ProgramTest routes to the program specified in the instruction's program_id
    init_mint_ix.program_id = spl_token_interface::ID;

    // 0.1) Create and initialize payer's source token account
    let user_token_kp = Keypair::new();
    let user_token = user_token_kp.pubkey();
    let token_acc_space = spl_token_interface::state::Account::LEN;
    let token_acc_lamports = rent.minimum_balance(token_acc_space);

    let create_user_token_ix = create_account(
        &payer,
        &user_token,
        token_acc_lamports,
        token_acc_space as u64,
        &spl_token_interface::ID,
    );

    let mut init_user_token_ix = initialize_account(
        &spl_token_interface::ID,
        &user_token,
        &mint,
        &payer, // owner of source token account
    )
    .unwrap();
    init_user_token_ix.program_id = spl_token_interface::ID;

    // 0.2) Create and initialize vault destination token account owned by the vault PDA
    let vault_token_kp = Keypair::new();
    let vault_token = vault_token_kp.pubkey();
    let create_vault_token_ix = create_account(
        &payer,
        &vault_token,
        token_acc_lamports,
        token_acc_space as u64,
        &spl_token_interface::ID,
    );

    let mut init_vault_token_ix = initialize_account(
        &spl_token_interface::ID,
        &vault_token,
        &mint,
        &vault, // owner of vault token account (PDA)
    )
    .unwrap();
    init_vault_token_ix.program_id = spl_token_interface::ID;

    // 0.3) Mint starting balance to payer's source token account
    let mut mint_to_ix = spl_token_interface::instruction::mint_to(
        &spl_token_interface::ID,
        &mint,
        &user_token,
        &payer,
        &[],
        STARTING_BALANCE,
    )
    .unwrap();
    mint_to_ix.program_id = spl_token_interface::ID;

    // Send setup transaction for mint + accounts + mint_to
    let setup_tx = Transaction::new_signed_with_payer(
        &[
            create_mint_ix,
            create_user_token_ix,
            create_vault_token_ix,
            init_mint_ix,
            init_user_token_ix,
            init_vault_token_ix,
            mint_to_ix,
        ],
        Some(&payer),
        &[&context.payer, &mint_kp, &user_token_kp, &vault_token_kp],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(setup_tx)
        .await
        .unwrap();

    // Assert initial SPL token balances
    let user_token_acc_before = context
        .banks_client
        .get_account(user_token)
        .await
        .unwrap()
        .expect("user token account must exist");
    let user_token_state_before = Account::unpack(&user_token_acc_before.data).unwrap();
    assert_eq!(user_token_state_before.amount, STARTING_BALANCE);

    let vault_token_acc_before = context
        .banks_client
        .get_account(vault_token)
        .await
        .unwrap()
        .expect("vault token account must exist");
    let vault_token_state_before = Account::unpack(&vault_token_acc_before.data).unwrap();
    assert_eq!(vault_token_state_before.amount, 0);

    // 1) Initialize Ephemeral ATA
    let ix_init_ata = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_EPHEMERAL_ATA, bump_ata],
    };

    // 2) Initialize Global Vault
    let ix_init_vault = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(payer, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: vec![instruction::INITIALIZE_GLOBAL_VAULT, bump_vault],
    };

    // Send both initializations in one tx
    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_ata, ix_init_vault],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    // 3) Deposit amount from payer's token to vault's token and increment Ephemeral ATA amount
    let amount: u64 = 100 * 10u64.pow(DECIMALS as u32);
    let mut data = vec![instruction::DEPOSIT_SPL_TOKENS];
    data.extend_from_slice(&amount.to_le_bytes());

    let ix_deposit = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new(ephemeral_ata, false), // [writable] Ephemeral ATA data
            AccountMeta::new_readonly(vault, false), // [] Global vault data
            AccountMeta::new_readonly(mint, false), // [] Mint pubkey (seed/consistency)
            AccountMeta::new(user_token, false),    // [writable] user source token acc
            AccountMeta::new(vault_token, false),   // [writable] vault token acc
            AccountMeta::new_readonly(payer, true), // [signer] user authority
            AccountMeta::new_readonly(spl_token_interface::ID, false), // [] token program id (readonly)
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix_deposit],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    // Assert SPL token balances after deposit
    let user_token_acc_after = context
        .banks_client
        .get_account(user_token)
        .await
        .unwrap()
        .expect("user token account must exist after deposit");
    let user_token_state_after = Account::unpack(&user_token_acc_after.data).unwrap();
    assert_eq!(user_token_state_after.amount, STARTING_BALANCE - amount);

    let vault_token_acc_after = context
        .banks_client
        .get_account(vault_token)
        .await
        .unwrap()
        .expect("vault token account must exist after deposit");
    let vault_token_state_after = Account::unpack(&vault_token_acc_after.data).unwrap();
    assert_eq!(vault_token_state_after.amount, amount);

    // Read back the Ephemeral ATA and verify amount incremented
    let account = context
        .banks_client
        .get_account(ephemeral_ata)
        .await
        .unwrap()
        .expect("ephemeral ata account must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), EphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let ata_data = unsafe { load_mut_unchecked::<EphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert_eq!(ata_data.amount, amount);
}
