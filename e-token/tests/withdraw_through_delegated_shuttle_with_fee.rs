use borsh::BorshDeserialize;
use bytemuck::Zeroable;
use dlp_api::{
    args::{MaybeEncryptedAccountMeta, MaybeEncryptedPubkey, PostDelegationActions},
    state::DelegationRecord,
};
use ephemeral_spl_api::{
    instruction::ESplInstruction,
    instructions::DepositAndDelegateShuttleArgs,
    state::transfer_queue::{TransferQueue, TransferQueueHeader, HEADER_LEN},
    ID as PROGRAM,
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_test::{tokio, ProgramTestContext};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::{InstructionError, Transaction, TransactionError};
use wheels::layout::Encodable as _;

mod common;
mod utils;

const DECIMALS: u8 = 6;
const AMOUNT: u64 = 100_000_000;
const FEE: u64 = 200_000;
const SHUTTLE_ID: u32 = 11;

/// (key, is_signer, is_writable) per account, then data.
type Action = (Pubkey, Vec<(Pubkey, bool, bool)>, Vec<u8>);

struct Fixture {
    context: ProgramTestContext,
    payer: Keypair,
    owner: Keypair,
    mint: Pubkey,
    queue: Pubkey,
}

impl Fixture {
    async fn new(label: &str) -> Self {
        let owner = utils::test_keypair(&format!("{label}::owner"));
        let mint_kp = utils::test_keypair(&format!("{label}::mint"));
        let validator = utils::test_pubkey(&format!("{label}::validator"));
        let (queue, bump) = TransferQueue::find_pda(&mint_kp.pubkey(), &validator);
        let mut header = TransferQueueHeader::zeroed();
        header.bump = bump;
        header.validator = validator;
        let mut context = utils::start_program_test_with(PROGRAM, |pt| {
            pt.add_account(
                queue,
                Account {
                    lamports: 1_000_000_000,
                    data: bytemuck::bytes_of(&header)[..HEADER_LEN].to_vec(),
                    owner: ephemeral_spl_api::program::DELEGATION_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            );
        })
        .await;
        let payer = utils::fixed_payer_keypair();
        utils::setup_mint_and_token_accounts(&mut context, &payer, &mint_kp, DECIMALS, 0, 1).await;

        let rent_pda = Pubkey::find_program_address(&[b"rent"], &PROGRAM).0;
        let mut fx = Self {
            context,
            payer,
            owner,
            mint: mint_kp.pubkey(),
            queue,
        };
        let setup = [
            Instruction {
                program_id: PROGRAM,
                accounts: vec![
                    AccountMeta::new(fx.payer.pubkey(), true),
                    AccountMeta::new(rent_pda, false),
                    AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                ],
                data: ESplInstruction::InitializeRentPda.to_vec(),
            },
            transfer(&fx.payer.pubkey(), &rent_pda, 100_000_000),
            Instruction {
                program_id: utils::associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(fx.payer.pubkey(), true),
                    AccountMeta::new(fx.ata(fx.owner.pubkey()), false),
                    AccountMeta::new_readonly(fx.owner.pubkey(), false),
                    AccountMeta::new_readonly(fx.mint, false),
                    AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                    AccountMeta::new_readonly(spl_token_interface::ID, false),
                ],
                data: vec![1],
            },
        ];
        fx.send(&setup, "wd_shuttle_fee::setup").await.unwrap();
        fx
    }

    fn ata(&self, wallet: Pubkey) -> Pubkey {
        utils::derive_associated_token_address(wallet, self.mint)
    }

    fn shuttle(&self) -> (Pubkey, Pubkey) {
        let metadata = utils::derive_shuttle_ephemeral_ata(PROGRAM, self.owner.pubkey(), self.mint, SHUTTLE_ID).0;
        (metadata, utils::derive_shuttle_eata(PROGRAM, metadata, self.mint).0)
    }

    fn delegation_record(&self) -> Pubkey {
        let dlp = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
        Pubkey::find_program_address(&[b"delegation", self.shuttle().1.as_ref()], &dlp).0
    }

    /// Instruction 26, or instruction 37 paying `fee` to the queue vault.
    fn withdraw_ix(&self, fee: Option<u64>) -> Instruction {
        let (metadata, eata) = self.shuttle();
        let dlp = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
        let mut accounts = vec![
            AccountMeta::new(self.payer.pubkey(), true),
            AccountMeta::new(Pubkey::find_program_address(&[b"rent"], &PROGRAM).0, false),
            AccountMeta::new(metadata, false),
            AccountMeta::new(eata, false),
            AccountMeta::new(self.ata(metadata), false),
            AccountMeta::new_readonly(self.owner.pubkey(), true),
            AccountMeta::new_readonly(PROGRAM, false),
            AccountMeta::new(
                Pubkey::find_program_address(&[b"buffer", eata.as_ref()], &PROGRAM).0,
                false,
            ),
            AccountMeta::new(self.delegation_record(), false),
            AccountMeta::new(
                Pubkey::find_program_address(&[b"delegation-metadata", eata.as_ref()], &dlp).0,
                false,
            ),
            AccountMeta::new_readonly(dlp, false),
            AccountMeta::new_readonly(utils::associated_token_program_id(), false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new(self.ata(self.owner.pubkey()), false),
            AccountMeta::new_readonly(self.mint, false),
            AccountMeta::new_readonly(spl_token_interface::ID, false),
        ];
        let mut data = DepositAndDelegateShuttleArgs {
            shuttle_id: SHUTTLE_ID,
            amount: AMOUNT,
            validator: None,
        }
        .encode()
        .unwrap();
        let discriminator = match fee {
            None => ESplInstruction::WithdrawThroughDelegatedShuttleWithMerge,
            Some(fee) => {
                accounts.push(AccountMeta::new_readonly(self.queue, false));
                data.extend_from_slice(&fee.to_le_bytes());
                ESplInstruction::WithdrawThroughDelegatedShuttleWithFee
            }
        };
        Instruction {
            program_id: PROGRAM,
            accounts,
            data: discriminator.with_data(&data),
        }
    }

    async fn send(&mut self, ixs: &[Instruction], label: &str) -> Result<(), TransactionError> {
        let owner = self.owner.insecure_clone();
        let signers: Vec<&Keypair> = if ixs
            .iter()
            .any(|ix| ix.accounts.iter().any(|a| a.pubkey == owner.pubkey() && a.is_signer))
        {
            vec![&self.payer, &owner]
        } else {
            vec![&self.payer]
        };
        let tx = Transaction::new_signed_with_payer(
            ixs,
            Some(&self.payer.pubkey()),
            &signers,
            self.context.banks_client.get_latest_blockhash().await.unwrap(),
        );
        common::metrics::process_transaction_with_metadata_recorded(&self.context.banks_client, tx, label)
            .await
            .unwrap()
            .result
    }

    /// The post-delegation actions stored behind the shuttle's delegation record.
    async fn stored_actions(&mut self) -> Vec<Action> {
        let record = self
            .context
            .banks_client
            .get_account(self.delegation_record())
            .await
            .unwrap()
            .expect("delegation record must exist");
        let actions =
            PostDelegationActions::deserialize(&mut &record.data[DelegationRecord::size_with_discriminator()..])
                .expect("stored post-delegation payload must decode");
        let keys: Vec<Pubkey> = actions
            .signers
            .iter()
            .copied()
            .chain(actions.non_signers.iter().map(|key| match key {
                MaybeEncryptedPubkey::ClearText(key) => *key,
                MaybeEncryptedPubkey::Encrypted(_) => panic!("withdraw actions are cleartext"),
            }))
            .map(Pubkey::new_from_array)
            .collect();
        let meta = |meta: &MaybeEncryptedAccountMeta| match meta {
            MaybeEncryptedAccountMeta::ClearText(meta) => {
                (keys[meta.key() as usize], meta.is_signer(), meta.is_writable())
            }
            MaybeEncryptedAccountMeta::Encrypted(_) => panic!("withdraw actions are cleartext"),
        };
        actions
            .instructions
            .iter()
            .map(|ix| {
                (
                    keys[ix.program_id as usize],
                    ix.accounts.iter().map(meta).collect(),
                    ix.data.prefix.clone(),
                )
            })
            .collect()
    }

    fn transfer_action(&self, destination: Pubkey, amount: u64) -> Action {
        let owner = self.owner.pubkey();
        let data = [&[12][..], &amount.to_le_bytes(), &[DECIMALS]].concat();
        let accounts = vec![
            (self.ata(owner), false, true),
            (self.mint, false, false),
            (destination, false, true),
            (owner, true, false),
        ];
        (spl_token_interface::ID, accounts, data)
    }
}

#[tokio::test]
async fn withdraw_with_fee_pays_the_queue_vault_before_funding_the_shuttle() {
    let mut fx = Fixture::new("withdraw_with_fee_moves_the_fee").await;
    fx.send(&[fx.withdraw_ix(Some(FEE))], "wd_shuttle_fee::withdraw")
        .await
        .unwrap();

    let payer = fx.payer.pubkey();
    assert_eq!(
        fx.send(
            &[transfer(&payer, &payer, 0), fx.withdraw_ix(Some(FEE))],
            "wd_shuttle_fee::duplicate"
        )
        .await
        .unwrap_err(),
        TransactionError::InstructionError(1, InstructionError::AccountAlreadyInitialized)
    );

    let actions = fx.stored_actions().await;
    assert_eq!(actions.len(), 4, "fee, shuttle funding, undelegate, close");
    assert_eq!(actions[0], fx.transfer_action(fx.ata(fx.queue), FEE));
    assert_eq!(actions[1], fx.transfer_action(fx.ata(fx.shuttle().0), AMOUNT - FEE));
    assert_eq!(
        actions[2].2[0],
        ESplInstruction::UndelegateAndCloseShuttleToOwner.value()
    );
    assert_eq!(actions[3].0, utils::magic_program_id());
}

#[tokio::test]
async fn withdraw_without_fee_keeps_instruction_26_actions() {
    let mut fx = Fixture::new("withdraw_without_fee_keeps_instruction_26_actions").await;
    fx.send(&[fx.withdraw_ix(None)], "wd_shuttle_fee::no_fee")
        .await
        .unwrap();

    let actions = fx.stored_actions().await;
    assert_eq!(actions.len(), 3, "shuttle funding, undelegate, close");
    assert_eq!(actions[0], fx.transfer_action(fx.ata(fx.shuttle().0), AMOUNT));
}

#[tokio::test]
async fn withdraw_with_fee_rejects_invalid_requests() {
    let mut fx = Fixture::new("withdraw_with_fee_rejects_invalid_requests").await;
    let mut missing_fee = fx.withdraw_ix(Some(FEE));
    missing_fee.data.truncate(missing_fee.data.len() - 8);
    let mut undelegated_queue = fx.withdraw_ix(Some(FEE));
    undelegated_queue.accounts[16].pubkey = fx.mint;

    let cases = [
        (fx.withdraw_ix(Some(0)), InstructionError::InvalidArgument),
        (fx.withdraw_ix(Some(AMOUNT)), InstructionError::InvalidArgument),
        (undelegated_queue, InstructionError::InvalidAccountOwner),
        (
            missing_fee,
            InstructionError::Custom(wheels::DataLayoutError::InvalidDataLength as u32),
        ),
    ];
    for (ix, expected) in cases {
        assert_eq!(
            fx.send(&[ix], "wd_shuttle_fee::rejects").await.unwrap_err(),
            TransactionError::InstructionError(0, expected)
        );
    }
}
