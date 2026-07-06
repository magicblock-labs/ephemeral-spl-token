use ephemeral_rollups_pinocchio::pda::ephemeral_balance_pda_from_payer;
use ephemeral_spl_api::{
    instructions::ExecuteQueuedTransferArgs, require, require_eq_keys, require_n_accounts,
    state::global_vault::GlobalVault,
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};
use pinocchio_system::{instructions::Transfer, ID as SYSTEM_PROGRAM_ID};
use pinocchio_token_2022::instructions::{CloseAccount, TransferChecked};
use solana_address::address_eq;
use spl_token_interface::ID as SPL_TOKEN_PROGRAM_ID;
use wheels::layout::Decodable as _;

use crate::processor::internal::{
    get_associated_token_address, read_mint_decimals,
    rent_pda::{RENT_PDA, RENT_PDA_BUMP, RENT_PDA_SEED},
    token_vault::validate_vault_for_mint,
    unwrap_pda::{NATIVE_MINT, UNWRAP_PDA, UNWRAP_PDA_BUMP, UNWRAP_PDA_SEED},
};

///
/// Executes on: BASE only.
///
/// Accounts:
///
///  0: []                  - PDA     : Global vault account.
///  1: []                  - SPL     : Mint account.
///  2: [writable]          - SPL     : Vault token account.
///  3: []                  - Any     : Destination owner. (writable when delivering native SOL)
///  4: [writable]          - SPL     : Destination token account.
///  5: [writable]          - PDA     : Global rent PDA.
///  6: []                  - SPL     : Token program.
///  7: []                  - SPL     : Associated token program.
///  8: []                  - Builtin : System program.
///
/// For native-mint (wrapped SOL) settlements the scheduler inserts two extra accounts before
/// DLP's appended trio; when present, the settlement unwraps and pays native lamports or
/// fails into the standard refund path (recipients never receive wrapped SOL):
///
///  9: [writable]          - SPL     : Unwrap scratch WSOL token account (ATA of the unwrap PDA).
/// 10: []                  - PDA     : Unwrap PDA (owner/authority of the scratch account).
///
/// Trailing accounts appended by DLP's CallHandlerV2:
///
///  *: []                  - Program : Source program (must equal this program).
///  *: []                  - PDA     : Queue PDA authority.
///  *: [signer]            - PDA     : Escrow signer PDA.
///
/// Instruction Data: ExecuteQueuedTransferArgs
///
#[inline(always)]
pub fn process_execute_ready_queued_transfer(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let (head_accounts, native_accounts, source_program, escrow_authority, escrow_signer) = match accounts.len() {
        12 => (&accounts[..9], None, &accounts[9], &accounts[10], &accounts[11]),
        14 => (
            &accounts[..9],
            Some((&accounts[9], &accounts[10])),
            &accounts[11],
            &accounts[12],
            &accounts[13],
        ),
        _ => return Err(ProgramError::NotEnoughAccountKeys),
    };

    let [
        vault_info, // force multi-line
        mint_info,
        vault_token_acc_info,
        destination_owner_info,
        destination_token_acc_info,
        rent_pda_info,
        token_program_info,
        associated_token_program_info,
        system_program_info,
    ] = require_n_accounts!(head_accounts, 9);

    let args = ExecuteQueuedTransferArgs::decode(instruction_data)?;

    // Note that accounts [source_program, escrow_authority, escrow_signer] are appended by DLP's
    // CallHandlerV2 instruction.
    require_eq_keys!(source_program.address(), &crate::ID, ProgramError::IncorrectAuthority);

    require!(escrow_signer.is_signer(), ProgramError::MissingRequiredSignature);

    let expected_escrow = ephemeral_balance_pda_from_payer(escrow_authority.address(), args.escrow_index());
    require_eq_keys!(&expected_escrow, escrow_signer.address(), ProgramError::InvalidSeeds);

    if let Some((scratch_wsol_ata_info, unwrap_pda_info)) = native_accounts {
        // Native-mint settlements pay native lamports or fail; a failure surfaces through the
        // action callback and triggers the standard refund path, so recipients never receive
        // wrapped SOL. The remaining failure case is an unfunded destination paid less than
        // the rent-exempt floor (runtime rent-exemption rule).
        process_native_delivery(
            NativeDeliveryAccounts {
                vault_info,
                mint_info,
                vault_token_acc_info,
                destination_owner_info,
                rent_pda_info,
                token_program_info,
                associated_token_program_info,
                system_program_info,
                scratch_wsol_ata_info,
                unwrap_pda_info,
            },
            args.amount(),
        )?;

        // Keep payment-confirmation logging identical to the legacy delivery path.
        if let Some(client_ref_id) = args.client_ref_id() {
            if client_ref_id != 0 {
                pinocchio_log::log!("client_ref_id: {}", client_ref_id);
            }
        }

        return Ok(());
    }

    if args.should_create_destination_ata_idempotent() {
        require!(
            rent_pda_info.owned_by(&SYSTEM_PROGRAM_ID),
            ProgramError::InvalidAccountOwner
        );
        require_eq_keys!(&RENT_PDA, rent_pda_info.address(), ProgramError::InvalidSeeds);
        require!(rent_pda_info.data_len() == 0, ProgramError::InvalidAccountData);
        require!(
            associated_token_program_info.address() == &pinocchio_associated_token_account::ID
                && system_program_info.address() == &SYSTEM_PROGRAM_ID,
            ProgramError::InvalidAccountData
        );

        let rent_bump_seed = [RENT_PDA_BUMP];
        let rent_signer_seed = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
        let rent_signer = Signer::from(&rent_signer_seed);

        (pinocchio_associated_token_account::instructions::CreateIdempotent {
            funding_account: rent_pda_info,
            account: destination_token_acc_info,
            wallet: destination_owner_info,
            mint: mint_info,
            system_program: system_program_info,
            token_program: token_program_info,
        })
        .invoke_signed(&[rent_signer])?;
    }

    let vault_bump = validate_vault_for_mint(vault_info, mint_info, vault_token_acc_info)?;
    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let vault_bump = [vault_bump];
    let signer_seeds = GlobalVault::signer_seeds(mint_info.address(), &vault_bump);
    let signer = Signer::from(&signer_seeds);

    TransferChecked {
        mint: mint_info,
        from: vault_token_acc_info,
        to: destination_token_acc_info,
        authority: vault_info,
        token_program: token_program_info.address(),
        amount: args.amount(),
        decimals,
    }
    .invoke_signed(&[signer])?;

    if let Some(client_ref_id) = args.client_ref_id() {
        if client_ref_id != 0 {
            pinocchio_log::log!("client_ref_id: {}", client_ref_id);
        }
    }

    Ok(())
}

struct NativeDeliveryAccounts<'a> {
    vault_info: &'a AccountView,
    mint_info: &'a AccountView,
    vault_token_acc_info: &'a AccountView,
    destination_owner_info: &'a AccountView,
    rent_pda_info: &'a AccountView,
    token_program_info: &'a AccountView,
    associated_token_program_info: &'a AccountView,
    system_program_info: &'a AccountView,
    scratch_wsol_ata_info: &'a AccountView,
    unwrap_pda_info: &'a AccountView,
}

/// Unwrap `amount` wrapped SOL from the vault and pay the recipient in native lamports.
///
/// The scratch WSOL account is created, filled with exactly `amount`, and closed within this
/// single instruction, so the rent PDA is net-neutral (its scratch rent is returned on close)
/// and the recipient receives exactly `amount` native lamports.
#[inline(never)]
fn process_native_delivery(accounts: NativeDeliveryAccounts<'_>, amount: u64) -> ProgramResult {
    let NativeDeliveryAccounts {
        vault_info,
        mint_info,
        vault_token_acc_info,
        destination_owner_info,
        rent_pda_info,
        token_program_info,
        associated_token_program_info,
        system_program_info,
        scratch_wsol_ata_info,
        unwrap_pda_info,
    } = accounts;

    // Native unwrap only applies to the classic SPL Token wrapped-SOL mint.
    require_eq_keys!(mint_info.address(), &NATIVE_MINT, ProgramError::InvalidAccountData);
    require!(
        address_eq(token_program_info.address(), &SPL_TOKEN_PROGRAM_ID),
        ProgramError::IncorrectProgramId
    );

    // Lamport credits are permissionless on Solana, so the destination is deliberately
    // unrestricted (base-layer semantics: the sender is responsible for the address). The one
    // structural failure left is the runtime's rent-exemption rule for unfunded destinations,
    // which bounces the settlement into the refund path.

    // Validate the rent PDA (funds the scratch account and sinks the unwrapped lamports).
    require!(
        rent_pda_info.owned_by(&SYSTEM_PROGRAM_ID),
        ProgramError::InvalidAccountOwner
    );
    require_eq_keys!(&RENT_PDA, rent_pda_info.address(), ProgramError::InvalidSeeds);
    require!(rent_pda_info.data_len() == 0, ProgramError::InvalidAccountData);
    require!(
        associated_token_program_info.address() == &pinocchio_associated_token_account::ID
            && system_program_info.address() == &SYSTEM_PROGRAM_ID,
        ProgramError::InvalidAccountData
    );

    // Validate the scratch account is the unwrap PDA's ATA for this mint.
    require_eq_keys!(unwrap_pda_info.address(), &UNWRAP_PDA, ProgramError::InvalidSeeds);
    let expected_scratch =
        get_associated_token_address(&UNWRAP_PDA, mint_info.address(), token_program_info.address());
    require_eq_keys!(
        &expected_scratch,
        scratch_wsol_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    let vault_bump = validate_vault_for_mint(vault_info, mint_info, vault_token_acc_info)?;
    let decimals = read_mint_decimals(mint_info, token_program_info)?;

    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seed = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seed);

    // 1. Create the scratch WSOL account, funded by the rent PDA.
    (pinocchio_associated_token_account::instructions::CreateIdempotent {
        funding_account: rent_pda_info,
        account: scratch_wsol_ata_info,
        wallet: unwrap_pda_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    })
    .invoke_signed(&[rent_signer])?;

    // 2. Move exactly `amount` wrapped SOL from the vault into the scratch account.
    let vault_bump = [vault_bump];
    let vault_signer_seeds = GlobalVault::signer_seeds(mint_info.address(), &vault_bump);
    let vault_signer = Signer::from(&vault_signer_seeds);
    TransferChecked {
        mint: mint_info,
        from: vault_token_acc_info,
        to: scratch_wsol_ata_info,
        authority: vault_info,
        token_program: token_program_info.address(),
        amount,
        decimals,
    }
    .invoke_signed(&[vault_signer])?;

    // 3. Close the scratch account, unwrapping its lamports (scratch rent + `amount`) to the rent PDA.
    let unwrap_bump_seed = [UNWRAP_PDA_BUMP];
    let unwrap_signer_seed = [Seed::from(UNWRAP_PDA_SEED), Seed::from(&unwrap_bump_seed)];
    let unwrap_signer = Signer::from(&unwrap_signer_seed);
    CloseAccount {
        account: scratch_wsol_ata_info,
        destination: rent_pda_info,
        authority: unwrap_pda_info,
        token_program: token_program_info.address(),
    }
    .invoke_signed(&[unwrap_signer])?;

    // 4. Forward exactly `amount` native lamports from the rent PDA to the recipient wallet,
    //    leaving the returned scratch rent with the rent PDA (net-neutral).
    let rent_signer = Signer::from(&rent_signer_seed);
    Transfer {
        from: rent_pda_info,
        to: destination_owner_info,
        lamports: amount,
    }
    .invoke_signed(&[rent_signer])?;

    Ok(())
}
