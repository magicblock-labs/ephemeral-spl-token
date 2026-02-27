use ephemeral_rollups_pinocchio::instruction::DelegateAccountCpiBuilder;
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, transfer_queue::TransferQueue,
};
use pinocchio::{error::ProgramError, AccountView, ProgramResult};

pub fn process_delegate_transfer_queue(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DelegateTransferQueueArgs::try_from_bytes(instruction_data)?;

    let [
        payer_info, // signer
        queue_info, // writable
        queue_eata_info, // writable
        owner_program, // []
        queue_buffer_acc, // writable
        queue_delegation_record, // writable
        queue_delegation_metadata, // writable
        eata_buffer_acc, // writable
        eata_delegation_record, // writable
        eata_delegation_metadata, // writable
        validator_info, // [optional]
        _delegation_program, // []
        system_program, // []
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    unsafe {
        if queue_info.owner().ne(&ephemeral_spl_api::program::ID) {
            return Err(ProgramError::IllegalOwner);
        }
    }

    // Check EATA
    let mint = {
        if !queue_eata_info.owned_by(&crate::ID) {
            return Err(ProgramError::IllegalOwner);
        }

        let queue_eata =
            unsafe { load_unchecked::<EphemeralAta>(queue_eata_info.borrow_unchecked())? };
        if queue_eata.owner != *queue_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }

        let bump = [args.eata_bump];
        let queue_eata_pda = ephemeral_spl_api::Address::create_program_address(
            &[
                queue_info.address().as_ref(),
                queue_eata.mint.as_ref(),
                bump.as_ref(),
            ],
            &crate::ID,
        )
        .map_err(|_| ProgramError::InvalidSeeds)?;
        if &queue_eata_pda != queue_eata_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        queue_eata.mint.clone()
    };

    // Check queue
    // It may not be initialized yet so we only check relevant data.
    let queue_bump = {
        if !queue_info.owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        let queue_bump = unsafe { queue_info.borrow_unchecked()[0] };
        let queue_pda = TransferQueue::create_address(&mint, &[queue_bump])?;
        if &queue_pda != queue_info.address() {
            return Err(ProgramError::InvalidSeeds);
        }

        queue_bump
    };

    let config = DelegateConfig {
        validator: if validator_info.address() == &crate::ID {
            Some(validator_info.address().clone())
        } else {
            None
        },
        ..DelegateConfig::default()
    };

    DelegateAccountCpiBuilder::new(
        payer_info,
        queue_info,
        owner_program,
        queue_buffer_acc,
        queue_delegation_record,
        queue_delegation_metadata,
        system_program,
    )
    .seeds(&TransferQueue::seeds(&mint))
    .bump(queue_bump)
    .config(config.clone())
    .invoke()?;

    DelegateAccountCpiBuilder::new(
        payer_info,
        queue_eata_info,
        owner_program,
        eata_buffer_acc,
        eata_delegation_record,
        eata_delegation_metadata,
        system_program,
    )
    .seeds(&[queue_info.address().as_ref(), mint.as_ref()])
    .bump(args.eata_bump)
    .config(config)
    .invoke()?;

    Ok(())
}

pub struct DelegateTransferQueueArgs {
    eata_bump: u8,
}

impl DelegateTransferQueueArgs {
    pub fn try_from_bytes(bytes: &[u8]) -> Result<&DelegateTransferQueueArgs, ProgramError> {
        if bytes.len() != 1 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(unsafe { &*(bytes.as_ptr() as *const DelegateTransferQueueArgs) })
    }
}
