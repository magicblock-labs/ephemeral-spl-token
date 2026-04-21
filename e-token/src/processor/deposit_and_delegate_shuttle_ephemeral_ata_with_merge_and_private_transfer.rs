#[cfg(feature = "logging")]
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use data_layout::variable_offset_layout;
use dlp_api::args::{
    EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
    MaybeEncryptedPubkey,
};
use dlp_api::compact::{self};

use ephemeral_spl_api::instruction::ESplInstruction;
use ephemeral_spl_api::state::transfer_queue::{queue_views, TransferQueue};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};

use crate::processor::{
    internal::shuttle_delegation::{
        merge_shuttle_into_token_account_action,
        process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
        undelegate_and_close_shuttle_action, DepositAndDelegateShuttleAccounts,
        DepositAndDelegateShuttleCommonArgs,
    },
    utils::read_mint_decimals,
};
use dlp_api::{args::PostDelegationActions, compact::ClearTextWithInsertable};

const BASIS_POINTS_DENOMINATOR: u128 = 10_000;
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

///
/// Executes on: BASE only.
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: [writable]          - PDA     : Rent PDA account.
///  2: [writable]          - PDA     : Shuttle metadata account.
///  3: [writable]          - PDA     : Shuttle EATA account.
///  4: [writable]          - SPL     : Shuttle wallet ATA account.
///  5: [signer]            - Keypair : Shuttle owner.
///  6: []                  - Program : Owner program.
///  7: [writable]          - PDA     : Buffer account.
///  8: [writable]          - PDA     : Delegation record account.
///  9: [writable]          - PDA     : Delegation metadata account.
/// 10: []                  - Program : Delegation program.
/// 11: []                  - SPL     : Associated token program.
/// 12: []                  - Builtin : System program.
/// 13: []                  - SPL     : Mint account.
/// 14: []                  - SPL     : Token program.
/// 15: []                  - PDA     : Global vault account.
/// 16: [writable]          - SPL     : Owner source token account.
/// 17: [writable]          - SPL     : Vault token account.
/// 18: [writable]          - PDA     : Transfer queue account.
///
/// Instruction Data: DepositAndDelegateShuttleWithPrivateTransferArgs
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        owner_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        _associated_token_program,
        system_program,
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
        queue_info,
    ] = require_n_accounts!(accounts, 19);

    let args = DepositAndDelegateShuttleWithPrivateTransferArgs::try_view_from(instruction_data)?;
    require!(args.amount() != 0, ProgramError::InvalidInstructionData);

    let common_accounts = DepositAndDelegateShuttleAccounts {
        payer_info,
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        owner_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
    };

    // require queue_info to already be delegated to the delegated program.
    require!(
        queue_info.owned_by(&ephemeral_spl_api::program::DELEGATION_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );

    #[cfg(feature = "logging")]
    {
        let shuttle = common_accounts.shuttle_info.address().to_string();
        let shuttle_eata = common_accounts.shuttle_eata_info.address().to_string();
        let shuttle_wallet = common_accounts
            .shuttle_wallet_ata_info
            .address()
            .to_string();
        let mint = common_accounts.mint_info.address().to_string();
        let owner_source = common_accounts
            .owner_source_token_info
            .address()
            .to_string();
        let vault_token = common_accounts.vault_token_info.address().to_string();
        let queue = queue_info.address().to_string();

        pinocchio_log::log!(
            "Private shuttle ix accounts shuttle={} shuttle_eata={} shuttle_wallet={} mint={} owner_source={} vault_token={} queue={}",
            shuttle.as_str(),
            shuttle_eata.as_str(),
            shuttle_wallet.as_str(),
            mint.as_str(),
            owner_source.as_str(),
            vault_token.as_str(),
            queue.as_str(),
        );
    }

    let (bump, validator) = {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views(data)?;
        (header.bump, header.validator)
    };
    let derived_queue =
        TransferQueue::derive_pda(common_accounts.mint_info.address(), &validator, bump)?;
    require_eq_keys!(
        &derived_queue,
        queue_info.address(),
        ProgramError::InvalidSeeds
    );

    let fee_amount = private_transfer_fee_amount(args.amount())?;
    let private_transfer_amount = args
        .amount()
        .checked_sub(fee_amount)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let mint_decimals = read_mint_decimals(
        common_accounts.mint_info,
        common_accounts.token_program_info,
    )?;

    let actions = {
        let private_transfer = private_transfer_action_encrypted(
            &common_accounts,
            queue_info,
            &args,
            private_transfer_amount,
        )?;

        alloc::vec![
            merge_shuttle_into_token_account_action(
                &common_accounts,
                common_accounts.owner_source_token_info,
            ),
            private_transfer_fee_action(&common_accounts, fee_amount, mint_decimals),
            undelegate_and_close_shuttle_action(&common_accounts),
        ]
        .cleartext_with_insertable(private_transfer, 1)
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        args.common_args()?,
        ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS,
        actions,
    )
}

#[variable_offset_layout]
pub struct DepositAndDelegateShuttleWithPrivateTransferArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    //
    // [capacity = 80] is because sealed-box encryption adds 48 bytes of overhead
    // irrespective of input bytes. So since encrypted_destination is encrypted
    // pubkey, its len has to be exactly: 32 + 48 = 80
    //
    // ref: https://github.com/jedisct1/libsodium-rs/blob/b3ad9336c0/src/crypto_box/mod.rs#L229-L232
    //
    pub encrypted_destination: [u8; 80],
    pub validator: Option<[u8; 32]>,
    #[flexible = 1]
    pub encrypted_data_suffix: Vec<u8>,
}

impl DepositAndDelegateShuttleWithPrivateTransferArgsView<'_> {
    fn common_args(&self) -> Result<DepositAndDelegateShuttleCommonArgs<'_>, ProgramError> {
        Ok(DepositAndDelegateShuttleCommonArgs {
            shuttle_id: self.shuttle_id(),
            amount: self.amount(),
            validator: self.validator(),
        })
    }
}

fn private_transfer_action_encrypted(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    args: &DepositAndDelegateShuttleWithPrivateTransferArgsView<'_>,
    amount: u64,
) -> Result<PostDelegationActions, ProgramError> {
    Ok(PostDelegationActions {
        inserted_signers: 0,
        inserted_non_signers: 0,
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()], // 0
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(crate::ID.to_bytes()), // 1
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()), // 2
            MaybeEncryptedPubkey::ClearText(common_accounts.global_vault_info.address().to_bytes()), // 3
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()), // 4
            MaybeEncryptedPubkey::ClearText(
                common_accounts.owner_source_token_info.address().to_bytes()
            ), // 5
            MaybeEncryptedPubkey::ClearText(common_accounts.vault_token_info.address().to_bytes()), // 6
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(
                args.encrypted_destination().into()
            )), // 7
            MaybeEncryptedPubkey::ClearText(
                common_accounts.token_program_info.address().to_bytes()
            ), // 8
            MaybeEncryptedPubkey::ClearText(
                common_accounts.shuttle_wallet_ata_info.address().to_bytes()
            ), // 9
        ],
        instructions: alloc::vec![MaybeEncryptedInstruction {
            program_id: 1,
            accounts: alloc::vec![
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(2, false)), // queue_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(3, false)), // global_vault_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(4, false)), // mint_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(5, false)), // owner_source_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(6, false)), // vault_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(7, false)), // destination_owner_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(0, true)), // owner_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(8, false)), // token_program_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(9, false)), // shuttle_wallet_ata_info
            ],
            data: MaybeEncryptedIxData {
                prefix: ESplInstruction::DepositAndQueueTransfer.with_data(&amount.to_le_bytes()),
                suffix: EncryptedBuffer::new(args.encrypted_data_suffix().into()),
            },
        }],
    })
}

#[inline(always)]
fn private_transfer_fee_amount(amount: u64) -> Result<u64, ProgramError> {
    Ok((amount as u128)
        .checked_mul(ephemeral_spl_api::consts::PRIVATE_TRANSFER_FEE_BASIS_POINTS as u128)
        .ok_or(ProgramError::InvalidInstructionData)?
        .checked_div(BASIS_POINTS_DENOMINATOR)
        .ok_or(ProgramError::InvalidInstructionData)? as u64)
}

fn private_transfer_fee_action(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    fee_amount: u64,
    mint_decimals: u8,
) -> Instruction {
    let mut data = alloc::vec![TRANSFER_CHECKED_DISCRIMINATOR];
    data.extend_from_slice(&fee_amount.to_le_bytes());
    data.push(mint_decimals);

    Instruction {
        program_id: *common_accounts.token_program_info.address(),
        accounts: alloc::vec![
            AccountMeta::new(*common_accounts.owner_source_token_info.address(), false),
            AccountMeta::new_readonly(*common_accounts.mint_info.address(), false),
            AccountMeta::new(*common_accounts.vault_token_info.address(), false),
            AccountMeta::new_readonly(*common_accounts.owner_info.address(), true),
        ],
        data,
    }
}
