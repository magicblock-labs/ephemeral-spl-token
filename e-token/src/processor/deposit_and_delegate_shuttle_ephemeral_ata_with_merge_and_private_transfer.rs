use alloc::vec::Vec;
use core::marker::PhantomData;
use core::ptr::read_unaligned;
use core::slice;
use dlp_api::args::{
    EncryptedBuffer, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
    MaybeEncryptedPubkey,
};
use dlp_api::compact::{self};

use ephemeral_spl_api::state::transfer_queue::QUEUE_SEED;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

use dlp_api::{args::PostDelegationActions, compact::ClearTextWithInsertable};

use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::undelegate_and_close_shuttle_action;
use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::{
    merge_shuttle_into_token_account_action, parse_deposit_and_delegate_shuttle_accounts,
    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions,
    DepositAndDelegateShuttleCommonArgs,
};

#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0..18 Same as DepositAndDelegateShuttleEphemeralAtaWithMerge
    // 19. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint]
    //
    let args = DepositAndDelegateShuttleWithPrivateTransferArgs::try_from_bytes(instruction_data)?;
    if args.amount() == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let common_accounts = parse_deposit_and_delegate_shuttle_accounts(accounts)?;
    let queue_info = accounts.get(19).ok_or(ProgramError::NotEnoughAccountKeys)?;

    let program_id = ephemeral_spl_api::program::id_address();
    let (derived_queue, _) = ephemeral_spl_api::Address::find_program_address(
        &[QUEUE_SEED, common_accounts.mint_info.address().as_ref()],
        &program_id,
    );
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !queue_info.owned_by(&ephemeral_spl_api::program::DELEGATION_PROGRAM_ID) {
        return Err(ProgramError::IllegalOwner);
    }

    let actions = {
        let private_transfer =
            private_transfer_action_encrypted(&common_accounts, queue_info, &args)?;

        alloc::vec![
            merge_shuttle_into_token_account_action(
                &common_accounts,
                common_accounts.owner_source_token_info,
            ),
            undelegate_and_close_shuttle_action(&common_accounts),
        ]
        .cleartext_with_insertable(private_transfer, 10)
    };

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        args.common_args()?,
        actions.into(),
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

    // decrypted { destination_token_account: pubkey }
    #[inline]
    fn encrypted_destination(&self) -> Result<&[u8], ProgramError> {
        unsafe { self.read_vardata::<1>() }
    }

    // decrypted { min_delay_ms: u64, max_delay_ms: 64, split: u32 } :: PACKED
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
    common_accounts: &crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    args: &DepositAndDelegateShuttleWithPrivateTransferArgs<'_>,
) -> Result<PostDelegationActions, ProgramError> {
    Ok(PostDelegationActions {
        inserted_signers: 0,
        inserted_non_signers: 0,
        signers: alloc::vec![common_accounts.owner_info.address().to_bytes()], // 0
        non_signers: alloc::vec![
            MaybeEncryptedPubkey::ClearText(ephemeral_spl_api::program::ID), // 1
            MaybeEncryptedPubkey::ClearText(queue_info.address().to_bytes()), // 2
            MaybeEncryptedPubkey::ClearText(common_accounts.global_vault_info.address().to_bytes()), // 3
            MaybeEncryptedPubkey::ClearText(common_accounts.mint_info.address().to_bytes()), // 4
            MaybeEncryptedPubkey::ClearText(
                common_accounts.owner_source_token_info.address().to_bytes()
            ), // 5
            MaybeEncryptedPubkey::ClearText(common_accounts.vault_token_info.address().to_bytes()), // 6
            MaybeEncryptedPubkey::Encrypted(
                EncryptedBuffer::new(args.encrypted_destination()?.into()) // common_accounts.destination_token_info.address().to_bytes()
            ), // 7
            MaybeEncryptedPubkey::ClearText(
                common_accounts.token_program_info.address().to_bytes()
            ), // 8
        ],
        instructions: alloc::vec![MaybeEncryptedInstruction {
            program_id: 1,
            accounts: alloc::vec![
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(2, false)), // queue_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(3, false)), // global_vault_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(4, false)), // mint_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(5, false)), // owner_source_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new(6, false)), // vault_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(7, false)), // destination_token_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(0, true)), // owner_info
                MaybeEncryptedAccountMeta::ClearText(compact::AccountMeta::new_readonly(8, false)), // token_program_info
            ],
            data: MaybeEncryptedIxData {
                prefix: {
                    let mut data_prefix = Vec::with_capacity(1 + 8);
                    data_prefix.push(ephemeral_spl_api::instruction::DEPOSIT_AND_QUEUE_TRANSFER);
                    data_prefix.extend_from_slice(&args.amount().to_le_bytes());
                    data_prefix
                },
                suffix: EncryptedBuffer::new(args.encrypted_data_suffix()?.into()),
            },
        }],
    })
}
