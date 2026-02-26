use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta;
use ephemeral_spl_api::state::{load_mut_unchecked, Initializable, RawType};
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use spl_token_interface::state::Account;
use {
    solana_instruction::AccountMeta, solana_program_test::tokio, solana_pubkey::Pubkey,
    solana_signer::Signer, solana_transaction::Transaction,
};

mod utils;

use crate::utils::setup_program_test;

#[tokio::test]
async fn initialize_shuttle_ephemeral_ata() {
    let pt = setup_program_test();
    let mut context = pt.start_with_context().await;

    let payer = context.payer.pubkey();
    let owner = Pubkey::new_unique();
    let mint_kp = Keypair::new();
    let mint = mint_kp.pubkey();
    let shuttle_id = 7_u32;

    let _setup =
        utils::setup_mint_and_token_accounts(&mut context, payer, &mint_kp, 6, 1_000, 1).await;

    let (shuttle_ephemeral_ata, bump) = utils::derive_shuttle_ephemeral_ata(
        ephemeral_spl_api::program::ID,
        owner,
        mint,
        shuttle_id,
    );
    let (shuttle_eata, _shuttle_eata_bump) =
        utils::derive_shuttle_eata(ephemeral_spl_api::program::ID, shuttle_ephemeral_ata, mint);
    let shuttle_wallet_ata = utils::derive_associated_token_address(&shuttle_ephemeral_ata, &mint);

    let mut data = vec![instruction::INITIALIZE_SHUTTLE_EPHEMERAL_ATA];
    data.extend_from_slice(&shuttle_id.to_le_bytes());
    data.push(bump);

    let ix = Instruction {
        program_id: ephemeral_spl_api::program::ID,
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

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer),
        &[&context.payer],
        context.last_blockhash,
    );
    context.banks_client.process_transaction(tx).await.unwrap();

    let account = context
        .banks_client
        .get_account(shuttle_ephemeral_ata)
        .await
        .unwrap()
        .expect("shuttle account must exist");

    assert_eq!(account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(account.data.len(), ShuttleEphemeralAta::LEN);

    let mut mut_acc = account.data.clone();
    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleEphemeralAta>(mut_acc.as_mut_slice()).unwrap() };
    assert!(shuttle.is_initialized());
    assert_eq!(shuttle.owner.as_array(), &owner.to_bytes());
    assert_eq!(shuttle.payer.as_array(), &payer.to_bytes());
    assert_eq!(shuttle.id, shuttle_id);

    let shuttle_eata_account = context
        .banks_client
        .get_account(shuttle_eata)
        .await
        .unwrap()
        .expect("shuttle eata account must exist");
    assert_eq!(shuttle_eata_account.owner, ephemeral_spl_api::program::ID);
    assert_eq!(shuttle_eata_account.data.len(), EphemeralAta::LEN);

    let mut mut_eata_acc = shuttle_eata_account.data.clone();
    let shuttle_eata_data =
        unsafe { load_mut_unchecked::<EphemeralAta>(mut_eata_acc.as_mut_slice()).unwrap() };
    assert_eq!(
        shuttle_eata_data.owner.as_array(),
        &shuttle_ephemeral_ata.to_bytes()
    );
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
