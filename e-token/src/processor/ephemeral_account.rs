use ephemeral_rollups_pinocchio::consts::MAGIC_PROGRAM_ID;
use pinocchio::{
    cpi::{invoke_signed_with_bounds, Signer},
    instruction::{InstructionAccount, InstructionView},
    AccountView, Address, ProgramResult,
};
use solana_pubkey::pubkey;

/// Vault account that collects rent for ephemeral accounts.
pub const MAGIC_VAULT_ID: Address = pubkey!("MagicVau1t999999999999999999999999999999999");

/// Bincode variant index for `MagicBlockInstruction::CreateEphemeralAccount` (variant 12).
const CREATE_EPHEMERAL_VARIANT: [u8; 4] = [12, 0, 0, 0];

/// Bincode variant index for `MagicBlockInstruction::CreateEphemeralAccount` (variant 12).
const RESIZE_EPHEMERAL_VARIANT: [u8; 4] = [13, 0, 0, 0];

/// Bincode variant index for `MagicBlockInstruction::CloseEphemeralAccount` (variant 14).
const CLOSE_EPHEMERAL_VARIANT: [u8; 4] = [14, 0, 0, 0];

/// Creates an ephemeral account via the magic program.
///
/// # Account references
/// - `sponsor`       `[WRITE, SIGNER]` Pays rent (can be a PDA)
/// - `account`       `[WRITE, SIGNER]` Ephemeral account to create (must have 0 lamports)
/// - `vault`         `[WRITE]`      Magic vault account (`EPHEMERAL_VAULT_ID`)
pub fn create_ephemeral_account(
    sponsor: &AccountView,
    account: &AccountView,
    vault: &AccountView,
    data_len: u32,
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    let mut data = [0u8; 8];
    data[..4].copy_from_slice(&CREATE_EPHEMERAL_VARIANT);
    data[4..].copy_from_slice(&data_len.to_le_bytes());

    let ix_accounts = [
        InstructionAccount::writable_signer(sponsor.address()),
        InstructionAccount::writable_signer(account.address()),
        InstructionAccount::writable(vault.address()),
    ];

    invoke_signed_with_bounds::<3>(
        &InstructionView {
            program_id: &MAGIC_PROGRAM_ID,
            accounts: &ix_accounts,
            data: &data,
        },
        &[sponsor, account, vault],
        signers,
    )
}

/// Creates an ephemeral account via the magic program.
///
/// # Account references
/// - `sponsor`       `[WRITE, SIGNER]` Pays rent (can be a PDA)
/// - `account`       `[WRITE]` Ephemeral account to create (must have 0 lamports)
/// - `vault`         `[WRITE]`      Magic vault account (`EPHEMERAL_VAULT_ID`)
pub fn resize_ephemeral_account(
    sponsor: &AccountView,
    account: &AccountView,
    vault: &AccountView,
    new_data_len: u32,
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    let mut data = [0u8; 8];
    data[..4].copy_from_slice(&CREATE_EPHEMERAL_VARIANT);
    data[4..].copy_from_slice(&new_data_len.to_le_bytes());

    let ix_accounts = [
        InstructionAccount::writable_signer(sponsor.address()),
        InstructionAccount::writable(account.address()),
        InstructionAccount::writable(vault.address()),
    ];

    invoke_signed_with_bounds::<3>(
        &InstructionView {
            program_id: &MAGIC_PROGRAM_ID,
            accounts: &ix_accounts,
            data: &data,
        },
        &[sponsor, account, vault],
        signers,
    )
}

/// Closes an ephemeral account via the magic program, refunding rent to the sponsor.
///
/// # Account references
/// - `sponsor`       `[WRITE, SIGNER]` Receives rent refund (can be a PDA)
/// - `account`       `[WRITE]`      Ephemeral account to close
/// - `vault`         `[WRITE]`      Magic vault account (`EPHEMERAL_VAULT_ID`)
pub fn close_ephemeral_account(
    sponsor: &AccountView,
    account: &AccountView,
    vault: &AccountView,
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    invoke_signed_with_bounds::<3>(
        &InstructionView {
            program_id: &MAGIC_PROGRAM_ID,
            accounts: &[
                InstructionAccount::writable_signer(sponsor.address()),
                InstructionAccount::writable(account.address()),
                InstructionAccount::writable(vault.address()),
            ],
            data: &CLOSE_EPHEMERAL_VARIANT,
        },
        &[sponsor, account, vault],
        signers,
    )
}
