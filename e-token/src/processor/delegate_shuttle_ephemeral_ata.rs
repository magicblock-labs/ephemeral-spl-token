use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

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
pub fn process_delegate_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
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

    let args = DelegateShuttleArgs::try_from_bytes(instruction_data)?;

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    require!(
        shuttle_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    // Loading the account to check if the shuttle is correctly initialized
    load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;

    require!(
        ephemeral_ata_info.owned_by(&crate::ID),
        ProgramError::InvalidAccountOwner
    );

    let (mint, eata_bump) = {
        let ephemeral_ata =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        require_eq_keys!(
            &ephemeral_ata.owner,
            shuttle_info.address(),
            ProgramError::InvalidAccountData
        );
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        (mint, ephemeral_ata.bump)
    };

    let derived_ephemeral_ata = EphemeralAta::derive_pda(shuttle_info.address(), &mint, eata_bump)?;
    require_eq_keys!(
        &derived_ephemeral_ata,
        ephemeral_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];

    let config = DelegateConfig {
        validator: args.validator().map(Address::new_from_array),
        ..DelegateConfig::default()
    };

    #[cfg(feature = "logging")]
    {
        pinocchio_log::log!("Delegating shuttle");
    }

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

//
// DataLayout:
//
//     00..32 : validator (optional [u8; 32])
//
// ValidLength:
//
//     00 | >= 32
//
pub struct DelegateShuttleArgs {
    validator: Option<[u8; 32]>,
}

impl DelegateShuttleArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateShuttleArgs, ProgramError> {
        if bytes.is_empty() {
            Ok(DelegateShuttleArgs { validator: None })
        } else if bytes.len() >= 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            Ok(DelegateShuttleArgs {
                validator: Some(arr),
            })
        } else {
            Err(ProgramError::InvalidInstructionData)
        }
    }

    #[inline]
    pub fn validator(&self) -> Option<[u8; 32]> {
        self.validator
    }
}
