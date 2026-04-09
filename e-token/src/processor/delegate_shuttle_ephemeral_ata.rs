use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_initialized, shuttle_ephemeral_ata::ShuttleMetadata,
};
use pinocchio::{address::address_eq, error::ProgramError, AccountView, Address, ProgramResult};

use crate::assert_owner;

pub fn process_delegate_shuttle_ephemeral_ata(
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
    let args = DelegateShuttleArgs::try_from_bytes(instruction_data)?;

    let [payer_info, shuttle_info, ephemeral_ata_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    assert_owner!(shuttle_info, &crate::ID);

    // Loading the account to check if the shuttle is correctly initialized
    load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;

    assert_owner!(ephemeral_ata_info, &crate::ID);

    let (mint, eata_bump) = {
        let ephemeral_ata =
            load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;
        if ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        (mint, ephemeral_ata.bump)
    };

    let derived_ephemeral_ata = EphemeralAta::derive_pda(shuttle_info.address(), &mint, eata_bump)?;
    if !address_eq(&derived_ephemeral_ata, ephemeral_ata_info.address()) {
        return Err(ProgramError::InvalidSeeds);
    }

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
