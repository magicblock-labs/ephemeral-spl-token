use ephemeral_spl_api::{
    instruction::ESplInstruction, instructions::ExecuteScheduledPrivateTransferArgs, state::stash::StashPda,
    ID as PROGRAM,
};
use hydra_api::{state::load_crank_mut, CRANK_HEADER_SIZE, CRANK_SEED_PREFIX};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_program_test::tokio;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::{InstructionError, Transaction, TransactionError};
use wheels::layout::Encodable as _;

mod utils;

/// ix 30 only runs the instruction its crank scheduled.
#[tokio::test]
async fn execute_scheduled_private_transfer_rejects_unscheduled_ix() {
    let (user, mint, shuttle_id) = (utils::test_pubkey("user"), utils::test_pubkey("mint"), 7u32);
    let (stash, stash_bump) = StashPda::find_pda(&user, &mint);
    let (rent_pda, _) = Pubkey::find_program_address(&[b"rent"], &PROGRAM);
    let mut seed = stash.to_bytes();
    seed[..4].copy_from_slice(&shuttle_id.to_le_bytes());
    let (crank, _) = Pubkey::find_program_address(&[CRANK_SEED_PREFIX, &seed], &hydra_api::ID);

    let key = |slot: usize| match slot {
        0 | 5 => stash,
        1 => rent_pda,
        6 => PROGRAM,
        13 => mint,
        14 => spl_token_interface::ID,
        16 => utils::derive_associated_token_address(stash, mint),
        20 => crank,
        _ => utils::test_pubkey(&format!("slot-{slot}")),
    };
    let metas: Vec<_> = (0..21)
        .map(|slot| match slot {
            6 | 10..=15 => AccountMeta::new_readonly(key(slot), false),
            _ => AccountMeta::new(key(slot), false),
        })
        .collect();
    let args = ExecuteScheduledPrivateTransferArgs {
        user,
        stash_bump,
        shuttle_id,
        validator: utils::test_pubkey("validator"),
        encrypted_destination: [1; 80],
        encrypted_data_suffix: vec![2; 16],
    };
    let scheduled = Instruction {
        program_id: PROGRAM,
        accounts: metas,
        data: ESplInstruction::ExecuteScheduledPrivateTransfer.with_data(&args.encode().unwrap()),
    };

    let mut crank_data = vec![0; CRANK_HEADER_SIZE];
    crank_data.extend((scheduled.accounts.len() as u16).to_le_bytes());
    for meta in &scheduled.accounts {
        crank_data.push(if meta.is_writable { 2 } else { 0 });
        crank_data.extend(meta.pubkey.to_bytes());
    }
    crank_data.extend(PROGRAM.to_bytes());
    crank_data.extend((scheduled.data.len() as u16).to_le_bytes());
    crank_data.extend(&scheduled.data);
    let region_len = (crank_data.len() - CRANK_HEADER_SIZE) as u16;
    let header = unsafe { load_crank_mut(&mut crank_data).unwrap() };
    header.authority = rent_pda.to_bytes();
    header.seed = seed;
    header.set_region_len(region_len);
    header.authority_signer = 1;

    let context = utils::start_program_test_with(PROGRAM, |pt| {
        pt.add_account(
            crank,
            Account {
                lamports: 1_000_000_000,
                data: crank_data,
                owner: hydra_api::ID,
                executable: false,
                rent_epoch: 0,
            },
        );
    })
    .await;
    let payer = utils::fixed_payer_keypair();
    let send = |ix: Instruction| {
        let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], context.last_blockhash);
        context.banks_client.process_transaction(tx)
    };

    let mut other_data = scheduled.clone();
    other_data.data = ESplInstruction::ExecuteScheduledPrivateTransfer.with_data(
        &ExecuteScheduledPrivateTransferArgs {
            encrypted_destination: [9; 80],
            ..args
        }
        .encode()
        .unwrap(),
    );
    let mut other_account = scheduled.clone();
    other_account.accounts[18].pubkey = utils::test_pubkey("other-queue");
    for mismatched in [other_data, other_account] {
        assert_eq!(
            send(mismatched).await.unwrap_err().unwrap(),
            TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
        );
    }

    // The scheduled ix itself still runs (empty stash: closes and returns).
    send(scheduled).await.unwrap();
}
