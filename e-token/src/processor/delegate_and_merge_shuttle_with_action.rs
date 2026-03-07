use alloc::vec;
use alloc::vec::Vec;
use borsh::BorshDeserialize;
use ephemeral_rollups_pinocchio::{
    consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    instruction::{
        AccountMeta as CompactAccountMeta, ClearTextWithInsertable,
        DelegateAccountWithActionsCpiBuilder, MaybeEncryptedAccountMeta, MaybeEncryptedInstruction,
        MaybeEncryptedIxData, MaybeEncryptedPubkey, PostDelegationActions,
    },
    types::DelegateConfig,
};
use ephemeral_spl_api::{
    args::DelegateShuttleWithActionArgs,
    instruction,
    state::{
        ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
        Initializable,
    },
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use solana_instruction::{AccountMeta, Instruction};

use crate::utils::read_token_account;

pub fn process_delegate_and_merge_shuttle_with_action(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DelegateShuttleWithActionArgs::try_from_slice(instruction_data)
        .map_err(|_| ProgramError::BorshIoError)?;

    let [
        owner_info,           // [signer]   Owner (the EATA owner)
        owner_ata_info,       // [writable] Owner ATA account (ATA for [owner, mint])
        shuttle_info,         // []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
        shuttle_ata_info,     // [writable] Shuttle ATA account (ATA for [shuttle_metadata, mint])
        shuttle_eata_info,    // [writable] Shuttle EATA account (PDA [shuttle_metadata, mint]) - signer via seeds
        owner_program,        // []         Owner program (the program owning the delegated PDA)
        buffer_acc,           // [writable] Buffer account (used by the delegation program)
        delegation_record,    // [writable] Delegation record account
        delegation_metadata,  // [writable] Delegation metadata account
        _delegation_program,  // []         Delegation program
        token_program_info,   // []         Token program
        system_program,       // []         System program
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if shuttle_eata_info.owned_by(&delegation_program) {
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

    unsafe {
        if shuttle_eata_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let mint = {
        let ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(shuttle_eata_info.borrow_unchecked())? };
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

    let (owner_ata_mint, owner_ata_owner, _owner_ata_amount) = read_token_account(owner_ata_info)?;
    if owner_ata_owner != *owner_info.address() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    if owner_ata_mint != mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let (shuttle_wallet_mint, shuttle_wallet_owner, _shuttle_wallet_amount) =
        read_token_account(shuttle_ata_info)?;
    if shuttle_wallet_owner != *shuttle_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle_wallet_mint != mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let bump = args.bump;
    let bump_seed = [bump];
    let derived_ephemeral_ata = ephemeral_spl_api::Address::create_program_address(
        &[
            shuttle_info.address().as_ref(),
            mint.as_ref(),
            bump_seed.as_ref(),
        ],
        &ephemeral_spl_api::program::id_address(),
    )
    .map_err(|_| ProgramError::InvalidSeeds)?;
    if derived_ephemeral_ata != *shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];

    let config = DelegateConfig {
        validator: args.validator.map(Address::new_from_array),
        ..DelegateConfig::default()
    };

    let actions = vec![
        Instruction {
            program_id: crate::ID.into(),
            accounts: vec![
                AccountMeta::new(*owner_info.address(), true),
                AccountMeta::new(*owner_ata_info.address(), false),
                AccountMeta::new(*shuttle_info.address(), false),
                AccountMeta::new(*shuttle_ata_info.address(), false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(*token_program_info.address(), false),
            ],
            data: vec![instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA],
        },
        Instruction {
            program_id: crate::ID.into(),
            accounts: vec![
                AccountMeta::new(*owner_info.address(), true),
                AccountMeta::new_readonly(*shuttle_info.address(), false),
                AccountMeta::new(*shuttle_eata_info.address(), false),
                AccountMeta::new(*shuttle_ata_info.address(), false),
                AccountMeta::new_readonly(*token_program_info.address(), false),
                AccountMeta::new(MAGIC_CONTEXT_ID, false),
                AccountMeta::new_readonly(MAGIC_PROGRAM_ID, false),
            ],
            data: vec![instruction::UNDELEGATE_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA],
        },
    ]
    .cleartext_with_insertable(args.encrypted_actions, 1);

    #[cfg(feature = "logging")]
    {
        pinocchio_log::log!("Delegating shuttle");
    }

    DelegateAccountWithActionsCpiBuilder::new(
        owner_info,
        shuttle_eata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(bump)
    .config(config)
    .actions(actions)
    .action_signer_accounts(&[owner_info])
    .invoke()
}
