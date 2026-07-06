pub(crate) mod callbacks;
pub(crate) mod ephemeral_account;
pub(crate) mod ephemeral_ata;
pub(crate) mod group_receipt;
pub(crate) mod group_receipt_accounts;
pub(crate) mod lamports_pda;
pub(crate) mod pda;
pub(crate) mod queue_authorized_action;
pub(crate) mod refund;
pub(crate) mod rent_pda;
pub(crate) mod shuttle_delegation;
pub(crate) mod token;
pub(crate) mod token_vault;
pub(crate) mod transfer_queue_refill;

pub(crate) use ephemeral_account::MAGIC_VAULT_ID;
#[cfg(feature = "logging")]
pub(crate) use group_receipt_accounts::group_receipt_log;
pub(crate) use group_receipt_accounts::{group_receipt_close, group_receipt_create, GroupReceiptAccounts};
pub(crate) use pda::CALLBACK_SIGNER;
use pinocchio::error::ProgramError;
use solana_address::Address;
pub(crate) use token::{
    get_associated_token_address, is_supported_token_program, read_mint_decimals, token_program_for_kind,
    token_program_kind, validate_token_account,
};
/// seed is created by overwriting the first 4-bytes of stash_pda with shuttle_id bytes
#[inline(always)]
pub(crate) fn derive_hydra_seed(stash_pda: &Address, shuttle_id: u32) -> [u8; 32] {
    let mut seed = *stash_pda.as_array();
    seed[..4].copy_from_slice(&shuttle_id.to_le_bytes());
    seed
}

#[inline(always)]
pub(crate) fn derive_ata(
    wallet: &Address,
    token_program: &Address,
    mint: &Address,
    bump_seed: u8,
) -> Result<Address, ProgramError> {
    let pda = Address::create_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref(), &[bump_seed]],
        &pinocchio_associated_token_account::ID,
    )?;
    Ok(pda)
}
