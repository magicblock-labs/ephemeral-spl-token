use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

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
        &[
            shuttle_info.address().as_ref(),
            mint.as_ref(),
            bump.as_ref(),
        ],
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
    .bump(args.bump())
    .config(config)
    .invoke()
}

pub struct DelegateShuttleArgs {
    bump: u8,
    validator: Option<[u8; 32]>,
}

impl DelegateShuttleArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateShuttleArgs, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let bump = bytes[0];
        let rest = &bytes[1..];
        let validator = if rest.is_empty() {
            None
        } else if rest.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(rest);
            Some(arr)
        } else {
            return Err(ProgramError::InvalidInstructionData);
        };
        Ok(DelegateShuttleArgs { bump, validator })
    }

    #[inline]
    pub fn validator(&self) -> Option<[u8; 32]> {
        self.validator
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        self.bump
    }
}
