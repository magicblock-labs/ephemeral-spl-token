#[cfg(feature = "logging")]
use alloc::string::ToString;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ptr::read_unaligned;
use core::slice;
use dlp_api::args::{
    EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
    MaybeEncryptedPubkey,
};
use dlp_api::compact::{self};

use ephemeral_spl_api::state::transfer_queue::{queue_views, TransferQueue};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};

use dlp_api::{args::PostDelegationActions, compact::ClearTextWithInsertable};

use crate::assert_owner;
use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::undelegate_and_close_shuttle_action;
use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::{
    merge_shuttle_into_token_account_action,
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
    DepositAndDelegateShuttleAccounts, DepositAndDelegateShuttleCommonArgs,
};
use crate::processor::utils::read_mint_decimals;

const BASIS_POINTS_DENOMINATOR: u128 = 10_000;
const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0..17 Same as DepositAndDelegateShuttleEphemeralAtaWithMerge, except there is no
    //        cleartext destination ATA account in this outer instruction.
    // 18. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint, validator]
    //
    let args = DepositAndDelegateShuttleWithPrivateTransferArgs::try_from_bytes(instruction_data)?;
    if args.amount() == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let (common_accounts, queue_info) =
        parse_deposit_and_delegate_shuttle_private_transfer_accounts(accounts)?;

    assert_owner!(
        queue_info,
        &ephemeral_spl_api::program::DELEGATION_PROGRAM_ID
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
        TransferQueue::create_pda(common_accounts.mint_info.address(), &validator, bump)?;
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

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

///
/// DataLayout:
///
///     00..04 : shuttle_id (u32)
///     04..12 : amount (u64)
///     12.... : validator (Option<Pubkey>; 0: None,  32: Some(..))
///     ...... : encrypted_destination (&u[8]; len: buffer)
///     ...... : encrypted_data_suffix (&u[8]; len: buffer)
///
///
struct DepositAndDelegateShuttleWithPrivateTransferArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl DepositAndDelegateShuttleWithPrivateTransferArgs<'_> {
    #[inline]
    fn try_from_bytes(
        bytes: &[u8],
    ) -> Result<DepositAndDelegateShuttleWithPrivateTransferArgs<'_>, ProgramError> {
        // MIN_LEN is the sum of the legth of 3 mandatory fixed-size fields
        //
        const MIN_LEN: usize = 4 +  // shuttle_id 
            8 +  // amount
            1 +  // min len for optional validator 
            1 + 32 +  // min len for mandatory destination_ata
            1 + 8 + 8 + 4 // min len for mandatory (min_delay_ms + max_delay_ms + split) 
            ;

        if bytes.len() < MIN_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(DepositAndDelegateShuttleWithPrivateTransferArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            _data: PhantomData,
        })
    }

    #[inline]
    fn shuttle_id(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }

    #[inline]
    fn amount(&self) -> u64 {
        unsafe { read_unaligned(self.raw.add(4) as *const u64) }
    }

    #[inline]
    fn common_args(&self) -> Result<DepositAndDelegateShuttleCommonArgs, ProgramError> {
        Ok(DepositAndDelegateShuttleCommonArgs {
            shuttle_id: self.shuttle_id(),
            amount: self.amount(),
            validator: self.validator()?,
        })
    }

    #[inline]
    fn validator(&self) -> Result<Option<[u8; 32]>, ProgramError> {
        let data = unsafe { self.read_vardata::<0>()? };

        if data.is_empty() {
            return Ok(None);
        } else if data.len() != 32 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut validator = [0u8; 32];
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), validator.as_mut_ptr(), 32);
        }
        Ok(Some(validator))
    }

    // decrypted { destination_owner: pubkey }
    #[inline]
    fn encrypted_destination(&self) -> Result<&[u8], ProgramError> {
        unsafe { self.read_vardata::<1>() }
    }

    // decrypted { min_delay_ms: u64, max_delay_ms: u64, split: u32, client_ref_id?: u64 } :: PACKED
    // Legacy payloads may still append flags before client_ref_id; the inner
    // DepositAndQueueTransfer parser keeps that layout for backward compatibility.
    #[inline]
    fn encrypted_data_suffix(&self) -> Result<&[u8], ProgramError> {
        unsafe { self.read_vardata::<2>() }
    }

    unsafe fn read_vardata<const VARINDEX: usize>(&self) -> Result<&[u8], ProgramError> {
        let mut offset = 12; // index where first vardata starts
        let mut var = 0;
        while var < VARINDEX {
            if offset >= self.len {
                return Err(ProgramError::InvalidInstructionData);
            }

            let len = *self.raw.add(offset);

            offset += 1 + len as usize;
            var += 1;
        }
        if offset >= self.len {
            return Err(ProgramError::InvalidInstructionData);
        }

        let len = *self.raw.add(offset);

        if len == 0 {
            Ok(&[])
        } else if (offset + 1 + len as usize) <= self.len {
            Ok(slice::from_raw_parts(
                self.raw.add(offset + 1),
                len as usize,
            ))
        } else {
            Err(ProgramError::InvalidInstructionData)
        }
    }
}

fn private_transfer_action_encrypted(
    common_accounts: &DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    args: &DepositAndDelegateShuttleWithPrivateTransferArgs<'_>,
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
                args.encrypted_destination()?.into()
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
                prefix: {
                    let mut data_prefix = Vec::with_capacity(1 + 8);
                    data_prefix.push(ephemeral_spl_api::instruction::DEPOSIT_AND_QUEUE_TRANSFER);
                    data_prefix.extend_from_slice(&amount.to_le_bytes());
                    data_prefix
                },
                suffix: EncryptedBuffer::new(args.encrypted_data_suffix()?.into()),
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

fn parse_deposit_and_delegate_shuttle_private_transfer_accounts(
    accounts: &[AccountView],
) -> Result<(DepositAndDelegateShuttleAccounts<'_>, &AccountView), ProgramError> {
    let [payer_info, rent_pda_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, owner_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, _associated_token_program, system_program, mint_info, token_program_info, global_vault_info, owner_source_token_info, vault_token_info, queue_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    Ok((
        DepositAndDelegateShuttleAccounts {
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
        },
        queue_info,
    ))
}
