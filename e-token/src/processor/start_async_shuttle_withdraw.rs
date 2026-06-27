#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::compact::ClearText;
use ephemeral_spl_api::{
    debug_log,
    instructions::DepositAndDelegateShuttleArgs,
    require_n_accounts,
    state::{ephemeral_ata::EphemeralAta, load},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};
use wheels::layout::Decodable as _;

const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;

use crate::processor::internal::{
    read_mint_decimals,
    shuttle_delegation::{
        build_start_async_shuttle_close_instruction, delegate_sponsored_shuttle_with_post_actions,
        prepare_sponsored_shuttle_delegation, DepositAndDelegateShuttleCommonArgs,
    },
    validate_token_account,
};

struct StartAsyncShuttleWithdrawAccounts<'a> {
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

///
/// Executes on:
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
/// 13: [writable]          - SPL     : Owner token account.
/// 14: []                  - SPL     : Mint account.
/// 15: []                  - SPL     : Token program.
///
/// Instruction Data: DepositAndDelegateShuttleArgs
///
#[inline(never)]
pub fn process_start_async_shuttle_withdraw(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
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
        owner_token_info,
        mint_info,
        token_program_info,
    ] = require_n_accounts!(accounts, 16);

    let args = DepositAndDelegateShuttleArgs::decode(instruction_data)?;

    let accounts = StartAsyncShuttleWithdrawAccounts {
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
    };

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
        0,
    )?;

    debug_log!(
        "Shuttle wallet ata: {}",
        accounts.shuttle_wallet_ata_info.address().to_string().as_str()
    );

    debug_log!("Shuttle: {}", accounts.shuttle_info.address().to_string().as_str());

    if prepared.already_delegated {
        return Ok(());
    }

    validate_token_account(
        accounts.owner_token_info,
        &prepared.mint,
        Some(accounts.owner_info.address()),
        Some(accounts.token_program_info.address()),
    )?;
    validate_token_account(
        accounts.shuttle_wallet_ata_info,
        &prepared.mint,
        Some(accounts.shuttle_info.address()),
        Some(accounts.token_program_info.address()),
    )?;

    let decimals = read_mint_decimals(accounts.mint_info, accounts.token_program_info)?;
    let post_actions = alloc::vec![
        transfer_owner_tokens_into_shuttle_action(&accounts, args.amount(), decimals)?,
        build_start_async_shuttle_close_instruction(
            accounts.payer_info.address(),
            accounts.rent_pda_info.address(),
            accounts.shuttle_info.address(),
            accounts.shuttle_eata_info.address(),
            accounts.shuttle_wallet_ata_info.address(),
            accounts.owner_token_info.address(),
            accounts.token_program_info.address(),
            None,
        ),
    ];

    // Shuttle has been initialized above
    let shuttle_eata = load::<EphemeralAta>(unsafe { accounts.shuttle_eata_info.borrow_unchecked() })?;

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
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: args.shuttle_id(),
            total_amount: args.amount(),
            validator: args.validator(),
        },
        &prepared.mint,
        shuttle_eata.bump,
        post_actions.cleartext(),
    )
}

fn transfer_owner_tokens_into_shuttle_action(
    accounts: &StartAsyncShuttleWithdrawAccounts<'_>,
    amount: u64,
    decimals: u8,
) -> Result<Instruction, ProgramError> {
    let mut data = alloc::vec![TRANSFER_CHECKED_DISCRIMINATOR];
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);

    Ok(Instruction {
        program_id: *accounts.token_program_info.address(),
        accounts: alloc::vec![
            AccountMeta::new(*accounts.owner_token_info.address(), false),
            AccountMeta::new_readonly(*accounts.mint_info.address(), false),
            AccountMeta::new(*accounts.shuttle_wallet_ata_info.address(), false),
            AccountMeta::new_readonly(*accounts.owner_info.address(), true),
        ],
        data,
    })
}
