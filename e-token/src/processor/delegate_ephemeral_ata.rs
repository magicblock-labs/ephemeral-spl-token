use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::load_mut_unchecked;
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

pub fn process_delegate_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts (in order used below):
    // 0. [signer]   Payer (seed used to derive Ephemeral ATA PDA)
    // 1. [writable] Ephemeral ATA account (PDA derived from [payer, mint]) - signer via seeds
    // 2. []         Owner program (the program owning the delegated PDA)
    // 3. [writable] Buffer account (used by the delegation program)
    // 4. [writable] Delegation record account
    // 5. [writable] Delegation metadata account
    // 6. []         Delegation program
    // 7. []         System program

    let args = DelegateArgs::try_from_bytes(instruction_data)?;

    let [payer_info, ephemeral_ata_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Load Ephemeral ATA account
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };

    let config = DelegateConfig {
        validator: args.validator().map(Address::new_from_array),
        ..DelegateConfig::default()
    };

    #[allow(clippy::clone_on_copy)]
    let mint = ephemeral_ata.mint.clone();
    #[allow(clippy::clone_on_copy)]
    let owner = ephemeral_ata.owner.clone();
    let seeds: &[&[u8]] = &[owner.as_ref(), mint.as_ref()];

    #[cfg(feature = "logging")]
    {
        pinocchio_log::log!("Delegating eata");
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

pub struct DelegateArgs {
    bump: u8,
    validator: Option<[u8; 32]>,
}

impl DelegateArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateArgs, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let bump = bytes[0];
        let rest = &bytes[1..];
        let validator = if rest.is_empty() {
            None
        } else if rest.len() >= 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&rest[..32]);
            Some(arr)
        } else {
            return Err(ProgramError::InvalidInstructionData);
        };
        Ok(DelegateArgs { bump, validator })
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
