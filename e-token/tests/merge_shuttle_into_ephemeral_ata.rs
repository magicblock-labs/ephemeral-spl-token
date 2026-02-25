use ephemeral_spl_api::instruction;
use ephemeral_spl_api::program::ID;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, RawType};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {
    solana_program_test::{tokio, ProgramTest},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::Transaction,
};

mod utils;

pub const PROGRAM: Pubkey = Pubkey::new_from_array(ID);

const DECIMALS: u8 = 6;
const STARTING_BALANCE: u64 = 10_000 * 10u64.pow(DECIMALS as u32);

#[tokio::test]
async fn merge_shuttle_into_ephemeral_ata_transfers_to_owner_ata_and_keeps_shuttle_open() {
    let mut pt = ProgramTest::new("ephemeral_token_program", PROGRAM, None);
    utils::add_associated_token_program(&mut pt);
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let owner = payer;
    let shuttle_id = 42_u32;

    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();

    let (shuttle_ephemeral_ata, shuttle_bump) =
        utils::derive_shuttle_ephemeral_ata(PROGRAM, owner, mint, shuttle_id);
    let (shuttle_eata, _shuttle_eata_bump) =
        utils::derive_shuttle_eata(PROGRAM, shuttle_ephemeral_ata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(&shuttle_ephemeral_ata, &mint);

    let setup = utils::setup_mint_and_token_accounts(
        &mut context,
        payer,
        &mint_kp,
        DECIMALS,
        STARTING_BALANCE,
        1,
    )
    .await;
    let user_ata = setup.user_tokens[0];

    let mut shuttle_init_data = vec![instruction::INITIALIZE_SHUTTLE_EPHEMERAL_ATA];
    shuttle_init_data.extend_from_slice(&shuttle_id.to_le_bytes());
    shuttle_init_data.push(shuttle_bump);
    let ix_init_shuttle = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
        ],
        data: shuttle_init_data,
    };

    let tx_init = Transaction::new_signed_with_payer(
        &[ix_init_shuttle],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_init)
        .await
        .unwrap();

    let amount: u64 = 700 * 10u64.pow(DECIMALS as u32);
    let mut ix_fund_shuttle_wallet = spl_token_interface::instruction::transfer_checked(
        &spl_token_interface::ID,
        &user_ata,
        &mint,
        &shuttle_wallet_ata,
        &payer,
        &[],
        amount,
        DECIMALS,
    )
    .unwrap();
    ix_fund_shuttle_wallet.program_id = spl_token_interface::ID;

    let tx_fund_shuttle_wallet = Transaction::new_signed_with_payer(
        &[ix_fund_shuttle_wallet],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_fund_shuttle_wallet)
        .await
        .unwrap();

    let owner_ata_before_merge = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("owner ata must exist");
    let owner_ata_before_merge_state = Account::unpack(&owner_ata_before_merge.data).unwrap();

    let shuttle_wallet_ata_before_merge = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_ata_before_merge_state =
        Account::unpack(&shuttle_wallet_ata_before_merge.data).unwrap();
    assert_eq!(shuttle_wallet_ata_before_merge_state.amount, amount);

    let ix_merge = Instruction {
        program_id: PROGRAM,
        accounts: vec![
            AccountMeta::new_readonly(payer, true),
            AccountMeta::new(user_ata, false),
            AccountMeta::new_readonly(shuttle_ephemeral_ata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ],
        data: vec![instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA],
    };

    let tx_merge = Transaction::new_signed_with_payer(
        &[ix_merge],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context
        .banks_client
        .process_transaction(tx_merge)
        .await
        .unwrap();

    let owner_ata_after_merge = context
        .banks_client
        .get_account(user_ata)
        .await
        .unwrap()
        .expect("owner ata must exist");
    let owner_ata_after_merge_state = Account::unpack(&owner_ata_after_merge.data).unwrap();
    assert_eq!(
        owner_ata_after_merge_state.amount,
        owner_ata_before_merge_state.amount + amount
    );

    let shuttle_wallet_ata_account = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    let shuttle_wallet_ata_state = Account::unpack(&shuttle_wallet_ata_account.data).unwrap();
    assert_eq!(shuttle_wallet_ata_state.amount, 0);

    let shuttle_account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must still exist");
    assert_eq!(shuttle_account.owner, PROGRAM);
    assert_eq!(shuttle_account.data.len(), ShuttleEphemeralAta::LEN);
    let mut shuttle_data = shuttle_account.data.clone();
    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleEphemeralAta>(shuttle_data.as_mut_slice()).unwrap() };
    assert_eq!(shuttle.id, shuttle_id);
    assert_eq!(shuttle.owner.as_array(), &owner.to_bytes());
    assert_eq!(shuttle.payer.as_array(), &payer.to_bytes());
}
