use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::transfer_queue::{queue_views_checked, QUEUE_SEED};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

pub fn process_delegate_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer
    // 1. [writable] Transfer queue PDA derived from [QUEUE_SEED, mint]
    // 2. []         Mint account
    // 3. []         Owner program (this program)
    // 4. [writable] Buffer account
    // 5. [writable] Delegation record account
    // 6. [writable] Delegation metadata account
    // 7. []         Delegation program
    // 8. []         System program
    let args = DelegateTransferQueueArgs::try_from_bytes(instruction_data)?;

    let [payer_info, queue_info, mint_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, system_program, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let program_id = ephemeral_spl_api::program::id_address();
    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if !queue_info.owned_by(&program_id) && !queue_info.owned_by(&delegation_program) {
        return Err(ProgramError::IllegalOwner);
    }

    let (derived_queue, bump) = ephemeral_spl_api::Address::find_program_address(
        &[QUEUE_SEED, mint_info.address().as_ref()],
        &program_id,
    );
    if derived_queue != *queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    {
        let data = unsafe { queue_info.borrow_unchecked() };
        let (header, _) = queue_views_checked(data)?;
        if header.mint != *mint_info.address() || header.bump != bump {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    if queue_info.owned_by(&delegation_program) {
        return Ok(());
    }

    if owner_program.address() != &program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    let config = DelegateConfig {
        validator: args.validator().map(Address::new_from_array),
        ..DelegateConfig::default()
    };
    let seeds: &[&[u8]] = &[QUEUE_SEED, mint_info.address().as_ref()];

    DelegateAccountCpiBuilder::new(
        payer_info,
        queue_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(bump)
    .config(config)
    .invoke()
}

pub struct DelegateTransferQueueArgs {
    validator: Option<[u8; 32]>,
}

impl DelegateTransferQueueArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateTransferQueueArgs, ProgramError> {
        let validator = if bytes.is_empty() {
            None
        } else if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Some(arr)
        } else {
            return Err(ProgramError::InvalidInstructionData);
        };

        Ok(DelegateTransferQueueArgs { validator })
    }

    #[inline]
    pub fn validator(&self) -> Option<[u8; 32]> {
        self.validator
    }
}
