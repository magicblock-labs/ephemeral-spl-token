use ephemeral_spl_api::{
    instruction,
    state::{ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata, RawType},
    ID as PROGRAM,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_program_pack::Pack;
use solana_program_test::tokio;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_token_interface::state::Account;

mod common;
mod utils;

#[tokio::test]
async fn initialize_shuttle() {
    let mut context = utils::start_program_test(PROGRAM).await;

    let payer_kp = utils::fixed_payer_keypair();
    let payer = payer_kp.pubkey();
    let owner = utils::test_pubkey("initialize_shuttle::owner");
    let mint_kp = utils::test_keypair("initialize_shuttle::mint");
    let mint = mint_kp.pubkey();
    let shuttle_id = 7_u32;

    let _setup = utils::setup_mint_and_token_accounts(&mut context, &payer_kp, &mint_kp, 6, 1_000, 1).await;

    let (shuttle_ephemeral_ata, _) = utils::derive_shuttle_ephemeral_ata(PROGRAM, owner, mint, shuttle_id);
    let (shuttle_eata, _) = utils::derive_shuttle_eata(PROGRAM, shuttle_ephemeral_ata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(shuttle_ephemeral_ata, mint);

    let mut data = instruction::ESplInstruction::InitializeShuttle.to_vec();
    data.extend_from_slice(&shuttle_id.to_le_bytes());

    let ix = Instruction {
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
        data,
    };

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer), &[&payer_kp], context.last_blockhash);
    common::metrics::process_transaction_record_cu(&context.banks_client, tx, "init_shuttle::init")
        .await
        .unwrap();

    let account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");

    assert_eq!(account.owner, PROGRAM);
    assert_eq!(account.data.len(), ShuttleMetadata::LEN);

    let mut mut_acc = account.data.clone();
    let shuttle = load_initialized::<ShuttleMetadata>(mut_acc.as_mut_slice()).unwrap();
    assert_eq!(shuttle.owner.as_array(), &owner.to_bytes());
    assert_eq!(shuttle.payer.as_array(), &payer.to_bytes());
    assert_eq!(shuttle.id, shuttle_id);

    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata account must exist");
    assert_eq!(shuttle_eata_account.owner, PROGRAM);
    assert_eq!(shuttle_eata_account.data.len(), EphemeralAta::LEN);

    let mut mut_eata_acc = shuttle_eata_account.data.clone();
    let shuttle_eata_data = load_initialized::<EphemeralAta>(mut_eata_acc.as_mut_slice()).unwrap();
    assert_eq!(shuttle_eata_data.owner.as_array(), &shuttle_ephemeral_ata.to_bytes());
    assert_eq!(shuttle_eata_data.mint.as_array(), &mint.to_bytes());
    assert_eq!(shuttle_eata_data.amount, 0);

    let shuttle_wallet_ata_account = context
        .banks_client
        .get_account(shuttle_wallet_ata)
        .await
        .unwrap()
        .expect("shuttle wallet ata must exist");
    assert_eq!(shuttle_wallet_ata_account.owner, spl_token_interface::ID);
    let shuttle_wallet_ata_state = Account::unpack(&shuttle_wallet_ata_account.data).unwrap();
    assert_eq!(shuttle_wallet_ata_state.owner, shuttle_ephemeral_ata);
    assert_eq!(shuttle_wallet_ata_state.mint, mint);
}
