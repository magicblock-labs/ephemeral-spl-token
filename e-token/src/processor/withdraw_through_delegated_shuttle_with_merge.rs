use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_spl_api::instruction::internal;
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

#[cfg(feature = "logging")]
use crate::alloc::string::ToString;

use crate::processor::deposit_and_delegate_shuttle_ephemeral_ata_with_merge::{
    delegate_sponsored_shuttle_with_post_actions, prepare_sponsored_shuttle_delegation,
    read_mint_decimals, DepositAndDelegateShuttleArgs,
};

pub(crate) struct WithdrawThroughDelegatedShuttleAccounts<'a> {
    pub(crate) payer_info: &'a AccountView,
    pub(crate) rent_pda_info: &'a AccountView,
    pub(crate) shuttle_info: &'a AccountView,
    pub(crate) shuttle_eata_info: &'a AccountView,
    pub(crate) shuttle_wallet_ata_info: &'a AccountView,
    pub(crate) owner_info: &'a AccountView,
    pub(crate) owner_program: &'a AccountView,
    pub(crate) buffer_acc: &'a AccountView,
    pub(crate) delegation_record: &'a AccountView,
    pub(crate) delegation_metadata: &'a AccountView,
    pub(crate) system_program: &'a AccountView,
    pub(crate) owner_token_info: &'a AccountView,
    pub(crate) mint_info: &'a AccountView,
    pub(crate) token_program_info: &'a AccountView,
}

#[inline(never)]
pub fn process_withdraw_through_delegated_shuttle_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DepositAndDelegateShuttleArgs::try_from_bytes(instruction_data)?;
    let accounts = parse_withdraw_through_delegated_shuttle_accounts(accounts)?;

    let prepared = prepare_sponsored_shuttle_delegation(
        accounts.payer_info,
        accounts.rent_pda_info,
        accounts.shuttle_info,
        accounts.shuttle_eata_info,
        accounts.shuttle_wallet_ata_info,
        accounts.owner_info,
        accounts.mint_info,
        accounts.token_program_info,
        accounts.system_program,
        args.shuttle_id(),
        args.shuttle_bump(),
    )?;

    #[cfg(feature = "logging")]
    {
        let shuttle_wallet_ata = accounts.shuttle_wallet_ata_info.address().to_string();
        pinocchio_log::log!("Shuttle wallet ata: {}", shuttle_wallet_ata.as_str());

        let shuttle = accounts.shuttle_info.address().to_string();
        pinocchio_log::log!("Shuttle: {}", shuttle.as_str());
    }

    if prepared.already_delegated {
        return Ok(());
    }

    validate_token_accounts(&accounts, &prepared.mint)?;

    let decimals = read_mint_decimals(accounts.mint_info)?;
    let post_actions = alloc::vec![
        transfer_owner_tokens_into_shuttle_action(&accounts, args.amount(), decimals)?,
        undelegate_withdraw_and_close_shuttle_action(&accounts),
    ];

    delegate_sponsored_shuttle_with_post_actions(
        accounts.payer_info,
        accounts.rent_pda_info,
        accounts.shuttle_info,
        accounts.shuttle_eata_info,
        accounts.owner_info,
        accounts.owner_program,
        accounts.buffer_acc,
        accounts.delegation_record,
        accounts.delegation_metadata,
        accounts.system_program,
        args.common_args(),
        &prepared.mint,
        prepared.shuttle_eata_bump,
        prepared.rent_bump,
        post_actions,
    )
}

fn parse_withdraw_through_delegated_shuttle_accounts(
    accounts: &[AccountView],
) -> Result<WithdrawThroughDelegatedShuttleAccounts<'_>, ProgramError> {
    let [payer_info, rent_pda_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, owner_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, _associated_token_program, system_program, owner_token_info, mint_info, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    Ok(WithdrawThroughDelegatedShuttleAccounts {
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
        owner_token_info,
        mint_info,
        token_program_info,
    })
}

fn validate_token_accounts(
    accounts: &WithdrawThroughDelegatedShuttleAccounts<'_>,
    expected_mint: &ephemeral_spl_api::Address,
) -> ProgramResult {
    if !accounts
        .owner_token_info
        .owned_by(accounts.token_program_info.address())
        || !accounts
            .shuttle_wallet_ata_info
            .owned_by(accounts.token_program_info.address())
    {
        return Err(ProgramError::IllegalOwner);
    }

    let owner_token_data = unsafe { accounts.owner_token_info.borrow_unchecked() };
    if owner_token_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let owner_token = unsafe { TokenAccount::from_bytes_unchecked(owner_token_data) };
    if !owner_token.is_initialized()
        || owner_token.owner() != accounts.owner_info.address()
        || owner_token.mint() != expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let shuttle_wallet_data = unsafe { accounts.shuttle_wallet_ata_info.borrow_unchecked() };
    if shuttle_wallet_data.len() < TokenAccount::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    let shuttle_wallet = unsafe { TokenAccount::from_bytes_unchecked(shuttle_wallet_data) };
    if !shuttle_wallet.is_initialized()
        || shuttle_wallet.owner() != accounts.shuttle_info.address()
        || shuttle_wallet.mint() != expected_mint
    {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

fn transfer_owner_tokens_into_shuttle_action(
    accounts: &WithdrawThroughDelegatedShuttleAccounts<'_>,
    amount: u64,
    decimals: u8,
) -> Result<Instruction, ProgramError> {
    let mut data = alloc::vec![12];
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);

    Ok(Instruction {
        program_id: *accounts.token_program_info.address(),
        accounts: alloc::vec![
            AccountMeta::new(pubkey(accounts.owner_token_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.mint_info.address()), false),
            AccountMeta::new(pubkey(accounts.shuttle_wallet_ata_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.owner_info.address()), true),
        ],
        data,
    })
}

fn undelegate_withdraw_and_close_shuttle_action(
    accounts: &WithdrawThroughDelegatedShuttleAccounts<'_>,
) -> Instruction {
    Instruction {
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
        accounts: alloc::vec![
            AccountMeta::new(pubkey(accounts.payer_info.address()), true),
            AccountMeta::new(pubkey(accounts.rent_pda_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.shuttle_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.shuttle_eata_info.address()), false),
            AccountMeta::new(pubkey(accounts.shuttle_wallet_ata_info.address()), false),
            AccountMeta::new(pubkey(accounts.owner_token_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.mint_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.token_program_info.address()), false),
            AccountMeta::new(Pubkey::from(MAGIC_CONTEXT_ID.to_bytes()), false),
            AccountMeta::new_readonly(Pubkey::from(MAGIC_PROGRAM_ID.to_bytes()), false),
        ],
        data: alloc::vec![internal::UNDELEGATE_WITHDRAW_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA],
    }
}

#[inline(always)]
fn pubkey(address: &ephemeral_spl_api::Address) -> Pubkey {
    *address
}
