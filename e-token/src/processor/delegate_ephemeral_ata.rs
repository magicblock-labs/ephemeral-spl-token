use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::load_mut_unchecked;
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};

pub fn process_delegate_ephemeral_ata(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts (in order used below):
    // 0. []         Payer (seed used to derive Ephemeral ATA PDA)
    // 1. [writable] Ephemeral ATA account (PDA derived from [payer, mint]) - signer via seeds
    // 2. []         Owner program (the program owning the delegated PDA)
    // 3. []         Buffer account (used by the delegation program)
    // 4. []         Delegation record account
    // 5. []         Delegation metadata account
    // 6. []         System program

    let args = DelegateArgs::try_from_bytes(instruction_data)?;

    let [payer_info, ephemeral_ata_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, _system_program] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Load Ephemeral ATA account
    let ephemeral_ata = unsafe {
        load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_mut_data_unchecked())?
    };

    let config = DelegateConfig {
        validator: args.validator(),
        ..DelegateConfig::default()
    };

    #[allow(clippy::clone_on_copy)]
    let mint = ephemeral_ata.mint.clone();
    #[allow(clippy::clone_on_copy)]
    let owner = ephemeral_ata.owner.clone();
    let seeds: &[&[u8]] = &[owner.as_slice(), mint.as_slice()];

    #[cfg(feature = "logging")]
    {
        pinocchio::msg!("Delegating eata to: ");
        pinocchio::pubkey::log(
            &pinocchio::pubkey::Pubkey::try_from(args.validator().unwrap_or_default())
                .unwrap_or_default(),
        );
    }

    // Delegate Ephemeral ATA PDA
    ephemeral_rollups_pinocchio::instruction::delegate_account(
        &[
            payer_info,
            ephemeral_ata_info,
            owner_program,
            buffer_acc,
            delegation_record,
            delegation_metadata,
        ],
        seeds,
        args.bump(),
        config,
    )
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
