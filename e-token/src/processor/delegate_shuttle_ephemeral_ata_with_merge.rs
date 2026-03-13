use ephemeral_rollups_pinocchio::{
    instruction::{ClearText, DelegateAccountWithActionsCpiBuilder},
    types::DelegateConfig,
};
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::processor::delegate_shuttle_ephemeral_ata::DelegateShuttleArgs;

pub fn process_delegate_shuttle_ephemeral_ata_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts (in order used below):
    // 0. [signer]   Payer
    // 1. []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
    // 2. [writable] Shuttle EATA account (PDA [shuttle_metadata, mint]) - signer via seeds
    // 3. []         Owner program (the program owning the delegated PDA)
    // 4. [writable] Buffer account (used by the delegation program)
    // 5. [writable] Delegation record account
    // 6. [writable] Delegation metadata account
    // 7. []         Delegation program
    // 8. []         System program
    // 9. [signer]   Shuttle owner (post-delegation action signer)
    // 10. [writable] Destination SPL token account
    // 11. [writable] Shuttle wallet ATA
    // 12. []         Mint account
    // 13. []         Token program
    let args = DelegateShuttleArgs::try_from_bytes(instruction_data)?;

    let [payer_info, shuttle_info, ephemeral_ata_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, owner_info, destination_token_info, shuttle_wallet_ata_info, mint_info, token_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !owner_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    unsafe {
        if shuttle_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
    if !shuttle.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle.owner != *owner_info.address() {
        return Err(ProgramError::IncorrectAuthority);
    }

    unsafe {
        if ephemeral_ata_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let mint = {
        let ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked())? };
        if !ephemeral_ata.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        mint
    };

    let bump = [args.bump()];
    let derived_ephemeral_ata = ephemeral_spl_api::Address::create_program_address(
        &[shuttle_info.address().as_ref(), mint.as_ref(), bump.as_ref()],
        &ephemeral_spl_api::program::id_address(),
    )
    .map_err(|_| ProgramError::InvalidSeeds)?;
    if derived_ephemeral_ata != *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];
    let config = DelegateConfig {
        validator: args.validator().map(Address::new_from_array),
        ..DelegateConfig::default()
    };

    let actions = alloc::vec![Instruction {
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
        accounts: alloc::vec![
            AccountMeta::new_readonly(pubkey(owner_info.address()), true),
            AccountMeta::new(pubkey(destination_token_info.address()), false),
            AccountMeta::new_readonly(pubkey(shuttle_info.address()), false),
            AccountMeta::new(pubkey(shuttle_wallet_ata_info.address()), false),
            AccountMeta::new_readonly(pubkey(mint_info.address()), false),
            AccountMeta::new_readonly(pubkey(token_program_info.address()), false),
        ],
        data: alloc::vec![ephemeral_spl_api::instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA],
    }]
    .cleartext();
    let action_signer_accounts = [owner_info];

    DelegateAccountWithActionsCpiBuilder::new(
        payer_info,
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(args.bump())
    .config(config)
    .actions(actions)
    .action_signer_accounts(&action_signer_accounts)
    .invoke()
}

#[inline]
fn pubkey(address: &Address) -> Pubkey {
    Pubkey::from(address.to_bytes())
}
