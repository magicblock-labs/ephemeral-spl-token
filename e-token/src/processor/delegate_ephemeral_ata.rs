use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::require_n_accounts;
use ephemeral_spl_api::state::{ephemeral_ata::EphemeralAta, load_initialized};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

///
/// Executes on:
///
/// Accounts:
///
///  0: [signer]            - Keypair : Payer (seed used to derive the Ephemeral ATA PDA).
///  1: [writable]          - PDA     : Ephemeral ATA account (PDA derived from [payer, mint]).
///  2: []                  - Program : Owner program.
///  3: [writable]          - PDA     : Buffer account.
///  4: [writable]          - PDA     : Delegation record account.
///  5: [writable]          - PDA     : Delegation metadata account.
///  6: []                  - Program : Delegation program.
///  7: []                  - Builtin : System program.
///
/// Instruction Data: DelegateArgs
///
pub fn process_delegate_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        payer_info, // force multi-line
        ephemeral_ata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        _delegation_program,
        system_program,
    ] = require_n_accounts!(accounts, 8);

    let args = DelegateArgs::try_from_bytes(instruction_data)?;

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if ephemeral_ata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    // Load Ephemeral ATA account
    let ephemeral_ata =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })?;

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
    .bump(ephemeral_ata.bump)
    .config(config)
    .invoke()
}

///
/// DataLayout:
///
///     00..32 : validator (optional [u8; 32])
///
/// ValidLength:
///
///     00 | 32
///
pub struct DelegateArgs {
    validator: Option<[u8; 32]>,
}

impl DelegateArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateArgs, ProgramError> {
        if bytes.is_empty() {
            Ok(DelegateArgs { validator: None })
        } else if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            Ok(DelegateArgs {
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
