use alloc::vec::Vec;
use core::marker::PhantomData;

use ephemeral_spl_api::state::transfer_queue::QUEUE_SEED;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::processor::{
    deposit_and_delegate_shuttle_ephemeral_ata_with_merge::{
        default_post_delegation_actions, parse_deposit_and_delegate_shuttle_accounts,
        process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions, pubkey,
        DepositAndDelegateShuttleCommonArgs,
    },
    deposit_and_queue_transfer::validate_deposit_and_queue_transfer_params,
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
    // Instruction data:
    // [0..4] shuttle_id (u32 LE)
    // [4]    shuttle metadata bump
    // [5..13] deposit amount (u64 LE)
    // [13..21] min_delay_ms (u64 LE)
    // [21..29] max_delay_ms (u64 LE)
    // [29..33] split count (u32 LE)
    // [33..65] optional validator pubkey
    let args = DepositAndDelegateShuttleWithPrivateTransferArgs::try_from_bytes(instruction_data)?;
    validate_deposit_and_queue_transfer_params(
        args.amount(),
        args.min_delay_ms(),
        args.max_delay_ms(),
        args.split(),
    )?;

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

    let mut actions = default_post_delegation_actions(&common_accounts);
    actions.push(private_transfer_action(&common_accounts, queue_info, &args));

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &common_accounts,
        args.common_args(),
        actions,
    )
}

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
        if bytes.len() != 33 && bytes.len() != 65 {
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
    fn shuttle_bump(&self) -> u8 {
        unsafe { *self.raw.add(4) }
    }

    #[inline]
    fn amount(&self) -> u64 {
        self.read_u64(5)
    }

    #[inline]
    fn min_delay_ms(&self) -> u64 {
        self.read_u64(13)
    }

    #[inline]
    fn max_delay_ms(&self) -> u64 {
        self.read_u64(21)
    }

    #[inline]
    fn split(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(29), buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }

    #[inline]
    fn validator(&self) -> Option<[u8; 32]> {
        if self.len == 33 {
            return None;
        }

        let mut validator = [0u8; 32];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(33), validator.as_mut_ptr(), 32);
        }
        Some(validator)
    }

    #[inline]
    fn common_args(&self) -> DepositAndDelegateShuttleCommonArgs {
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: self.shuttle_id(),
            shuttle_bump: self.shuttle_bump(),
            amount: self.amount(),
            validator: self.validator(),
        }
    }

    #[inline]
    fn read_u64(&self, offset: usize) -> u64 {
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(offset), buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }
}

fn private_transfer_action(
    common_accounts: &crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::DepositAndDelegateShuttleAccounts<'_>,
    queue_info: &AccountView,
    args: &DepositAndDelegateShuttleWithPrivateTransferArgs<'_>,
) -> Instruction {
    let mut data = Vec::with_capacity(1 + 8 + 8 + 8 + 4);
    data.push(ephemeral_spl_api::instruction::DEPOSIT_AND_QUEUE_TRANSFER);
    data.extend_from_slice(&args.amount().to_le_bytes());
    data.extend_from_slice(&args.min_delay_ms().to_le_bytes());
    data.extend_from_slice(&args.max_delay_ms().to_le_bytes());
    data.extend_from_slice(&args.split().to_le_bytes());

    Instruction {
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
        accounts: alloc::vec![
            AccountMeta::new(pubkey(queue_info.address()), false),
            AccountMeta::new_readonly(pubkey(common_accounts.global_vault_info.address()), false),
            AccountMeta::new_readonly(pubkey(common_accounts.mint_info.address()), false),
            AccountMeta::new(
                pubkey(common_accounts.owner_source_token_info.address()),
                false
            ),
            AccountMeta::new(pubkey(common_accounts.vault_token_info.address()), false),
            AccountMeta::new_readonly(
                pubkey(common_accounts.destination_token_info.address()),
                false
            ),
            AccountMeta::new_readonly(pubkey(common_accounts.owner_info.address()), true),
            AccountMeta::new_readonly(pubkey(common_accounts.token_program_info.address()), false),
        ],
        data,
    }
}
