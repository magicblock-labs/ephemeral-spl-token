use ephemeral_rollups_pinocchio::{instruction::DelegateAccountCpiBuilder, types::DelegateConfig};
use ephemeral_spl_api::{
    debug_log,
    instructions::DelegateShuttleArgs,
    require, require_eq_keys, require_n_accounts,
    state::{ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata},
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};
use wheels::layout::Decodable as _;

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer.
///  1: []                  - PDA     : Shuttle metadata account (PDA derived from [owner, mint, shuttle_id]).
///  2: [writable]          - PDA     : Shuttle EATA account (PDA derived from [shuttle_metadata, mint]).
///  3: []                  - Program : Owner program.
///  4: [writable]          - PDA     : Buffer account.
///  5: [writable]          - PDA     : Delegation record account.
///  6: [writable]          - PDA     : Delegation metadata account.
///  7: []                  - Program : Delegation program.
///  8: []                  - Builtin : System program.
///
/// Instruction Data: DelegateShuttleArgs
///
pub fn process_delegate_shuttle_ephemeral_ata(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let [
        payer_info, // force multi-line
        shuttle_info,
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
    ] = require_n_accounts!(accounts, 9);

    let args = DelegateShuttleArgs::decode(instruction_data)?;

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    require!(shuttle_info.owned_by(&crate::ID), ProgramError::InvalidAccountOwner);

    // Loading the account to check if the shuttle is correctly initialized
    load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;

    require!(
        ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let (mint, eata_bump) = {
        let ephemeral_ata = load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        require_eq_keys!(
            &ephemeral_ata.owner,
            shuttle_info.address(),
            ProgramError::InvalidAccountData
        );
        (ephemeral_ata.mint, ephemeral_ata.bump)
    };

    let derived_ephemeral_ata = EphemeralAta::derive_pda(shuttle_info.address(), &mint, eata_bump)?;
    require_eq_keys!(
        &derived_ephemeral_ata,
        ephemeral_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];

    let config = DelegateConfig {
        validator: args.validator().copied(),
        ..DelegateConfig::default()
    };

    debug_log!("Delegating shuttle");

    DelegateAccountCpiBuilder::new(
        payer_info,
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(eata_bump)
    .config(config)
    .invoke()
}
