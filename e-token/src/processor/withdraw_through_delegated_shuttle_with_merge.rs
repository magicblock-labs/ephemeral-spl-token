#[cfg(feature = "logging")]
use alloc::string::ToString;

use dlp_api::compact::ClearText;
use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use ephemeral_spl_api::{
    debug_log,
    instructions::DepositAndDelegateShuttleArgs,
    require, require_eq_keys, require_n_accounts,
    state::{
        ephemeral_ata::EphemeralAta,
        load,
        transfer_queue::{queue_views, TransferQueue},
    },
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use wheels::layout::Decodable as _;

const TRANSFER_CHECKED_DISCRIMINATOR: u8 = 12;
/// Magic Program `CloseMagicAta` (bincode enum discriminant).
const CLOSE_MAGIC_ATA_DISCRIMINATOR: u32 = 26;

use crate::processor::internal::{
    get_associated_token_address, read_mint_decimals,
    shuttle_delegation::{
        build_undelegate_and_close_shuttle_instruction, delegate_sponsored_shuttle_with_post_actions,
        prepare_sponsored_shuttle_delegation, DepositAndDelegateShuttleCommonArgs,
    },
    validate_token_account,
};

struct WithdrawThroughDelegatedShuttleAccounts<'a> {
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
/// 16: []                  - PDA     : Delegated transfer queue (`with_fee` only).
///
/// Instruction Data: DepositAndDelegateShuttleArgs, then fee (u64 LE) when
/// `with_fee`; the fee goes to the queue vault ATA in the same ER transaction.
///
#[inline(never)]
pub fn process_withdraw_through_delegated_shuttle_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
    with_fee: bool,
) -> ProgramResult {
    let (accounts, fee_accounts) = accounts.split_at(accounts.len().min(16));
    let (instruction_data, fee) = match (with_fee, fee_accounts) {
        (false, []) => (instruction_data, None),
        (true, [queue_info]) => {
            let (data, fee) = instruction_data
                .split_last_chunk::<8>()
                .ok_or(ProgramError::InvalidInstructionData)?;
            (data, Some((u64::from_le_bytes(*fee), queue_info)))
        }
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };
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
    let fee = match fee {
        Some((fee, queue_info)) => {
            require!(fee > 0 && fee < args.amount(), ProgramError::InvalidArgument);
            require!(
                queue_info.owned_by(&ephemeral_spl_api::program::DELEGATION_PROGRAM_ID),
                ProgramError::InvalidAccountOwner
            );
            let (header, _) = queue_views(unsafe { queue_info.borrow_unchecked() })?;
            let queue = TransferQueue::derive_pda(mint_info.address(), &header.validator, header.bump)?;
            require_eq_keys!(&queue, queue_info.address(), ProgramError::InvalidSeeds);
            Some((
                fee,
                get_associated_token_address(&queue, mint_info.address(), token_program_info.address()),
            ))
        }
        None => None,
    };
    let shuttled_amount = args.amount() - fee.map_or(0, |(fee, _)| fee);

    let accounts = WithdrawThroughDelegatedShuttleAccounts {
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
        // Revert duplicates so the sponsor pays setup only once.
        require!(fee.is_none(), ProgramError::AccountAlreadyInitialized);
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
    // The close must come after the shuttle undelegation instruction: the
    // actions run as one ER transaction and the undelegation still validates
    // the owner token account, which the close removes once drained.
    let mut post_actions = alloc::vec![];
    if let Some((fee, queue_vault_token)) = fee {
        post_actions.push(transfer_owner_tokens_action(
            &accounts,
            &queue_vault_token,
            fee,
            decimals,
        ));
    }
    post_actions.extend([
        transfer_owner_tokens_action(
            &accounts,
            accounts.shuttle_wallet_ata_info.address(),
            shuttled_amount,
            decimals,
        ),
        build_undelegate_and_close_shuttle_instruction(
            accounts.payer_info.address(),
            accounts.rent_pda_info.address(),
            accounts.shuttle_info.address(),
            accounts.shuttle_eata_info.address(),
            accounts.shuttle_wallet_ata_info.address(),
            accounts.owner_token_info.address(),
            accounts.token_program_info.address(),
            None,
        ),
        close_magic_ata_source_action(&accounts),
    ]);

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
            total_amount: shuttled_amount,
            validator: args.validator(),
        },
        &prepared.mint,
        shuttle_eata.bump,
        post_actions.cleartext(),
    )
}

/// Closes the owner's ER-side source account when it is a fully drained
/// Magic ATA; the Magic Program no-ops in every other case, so this
/// action is safe on eATA-backed and partial withdrawals alike.
fn close_magic_ata_source_action(accounts: &WithdrawThroughDelegatedShuttleAccounts<'_>) -> Instruction {
    Instruction {
        program_id: MAGIC_PROGRAM_ID,
        accounts: alloc::vec![
            AccountMeta::new_readonly(*accounts.owner_info.address(), true),
            AccountMeta::new(*accounts.owner_token_info.address(), false),
        ],
        data: CLOSE_MAGIC_ATA_DISCRIMINATOR.to_le_bytes().to_vec(),
    }
}

fn transfer_owner_tokens_action(
    accounts: &WithdrawThroughDelegatedShuttleAccounts<'_>,
    destination: &Address,
    amount: u64,
    decimals: u8,
) -> Instruction {
    let mut data = alloc::vec![TRANSFER_CHECKED_DISCRIMINATOR];
    data.extend_from_slice(&amount.to_le_bytes());
    data.push(decimals);

    Instruction {
        program_id: *accounts.token_program_info.address(),
        accounts: alloc::vec![
            AccountMeta::new(*accounts.owner_token_info.address(), false),
            AccountMeta::new_readonly(*accounts.mint_info.address(), false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*accounts.owner_info.address(), true),
        ],
        data,
    }
}
