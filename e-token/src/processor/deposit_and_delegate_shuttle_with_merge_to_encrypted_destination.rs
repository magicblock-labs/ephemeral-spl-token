use dlp_api::{
    args::{
        EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
        MaybeEncryptedPubkey, PostDelegationActions,
    },
    compact::{self, ClearTextWithInsertable},
};
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::{
    instruction::ESplInstruction, instructions::DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs, require,
    require_n_accounts,
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use wheels::layout::Decodable as _;

use crate::processor::internal::shuttle_delegation::{
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions, undelegate_and_close_shuttle_action,
    DepositAndDelegateShuttleAccounts, DepositAndDelegateShuttleCommonArgs,
};

///
/// Executes on: BASE only.
///
/// Private base->ephemeral transfer with an encrypted destination: deposits the
/// amount into a shuttle EATA and delegates it with post-delegation actions
/// that, on the ER, create the destination's rent-pending ATA and merge the
/// shuttle balance into it (instruction 34), then undelegate and close the
/// shuttle. The destination owner and ATA are carried only as encrypted
/// buffers and never appear in cleartext on base.
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
///
/// Instruction Data: DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs
///
#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_with_merge_to_encrypted_destination(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DepositAndDelegateShuttleWithMergeToEncryptedDestinationArgs::decode(instruction_data)?;

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
    ] = require_n_accounts!(accounts, 18);

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

    let actions = {
        let ensure_and_merge = merge_to_encrypted_destination_action(
            &common_accounts,
            args.encrypted_destination_owner(),
            args.encrypted_destination_ata(),
        );

        alloc::vec![undelegate_and_close_shuttle_action(&common_accounts, None)]
            .cleartext_with_insertable(ensure_and_merge, 0)
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: args.shuttle_id(),
            total_amount: args.amount(),
            validator: args.validator(),
        },
        ephemeral_spl_api::consts::SPONSORED_SHUTTLE_MERGE_TO_ENCRYPTED_DESTINATION_EXTRA_LAMPORTS,
        actions,
    )
}

/// Build the encrypted instruction-33 action: the destination owner and ATA
/// stay encrypted and are decrypted by the validator inside the ER.
fn merge_to_encrypted_destination_action(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    encrypted_destination_owner: &[u8; 80],
    encrypted_destination_ata: &[u8; 80],
) -> PostDelegationActions {
    PostDelegationActions {
        inserted_signers: 0,
        inserted_non_signers: 0,
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()],
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(crate::ID.to_bytes()),
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(encrypted_destination_owner.into())),
            MaybeEncryptedPubkey::Encrypted(EncryptedBuffer::new(encrypted_destination_ata.into())),
            MaybeEncryptedPubkey::ClearText(common_accounts.shuttle_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.shuttle_wallet_ata_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(common_accounts.token_program_info.address().to_bytes()),
            MaybeEncryptedPubkey::ClearText(MAGIC_PROGRAM_ID.to_bytes()),
        ],
        instructions: alloc::vec![MaybeEncryptedInstruction {
            program_id: 1,
            accounts: alloc::vec![
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(0, true)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(2, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(3, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(4, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(5, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(6, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(7, false)),
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(8, false)),
            ],
            data: MaybeEncryptedIxData {
                prefix: ESplInstruction::MergeShuttleIntoRentPendingAta.to_vec(),
                suffix: EncryptedBuffer::default(),
            },
        }],
    }
}
