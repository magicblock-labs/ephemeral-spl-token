use ephemeral_rollups_pinocchio::crank::CancelCrankCpi;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, transfer_queue::TransferQueue,
};
use pinocchio::{
    cpi::Signer,
    sysvars::{clock::Clock, Sysvar},
    Address,
};
use pinocchio_token_2022::{instructions::TransferChecked, state::Mint};

use {
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

#[inline(always)]
pub fn process_transfers(accounts: &[AccountView], instruction_data: &[u8]) -> ProgramResult {
    let args = ProcessTransfersArgs::try_from_bytes(instruction_data)?;

    let [
        mint_info, // readonly
        queue_info, // writable, PDA [mint]
        queue_ata_info, // writable, ATA for [depositor, mint]
        shuttle_info, // writable, shuttle PDA
        shuttle_ata_info, // writable, ATA for [shuttle, mint]
        shuttle_eata_info, // readonly, EATA for [shuttle, mint]
        magic_context_info, // writable, magic context PDA
        magic_program_info, // [] magic program ID
        token_program_info, // []
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let queue = unsafe { load_mut_unchecked::<TransferQueue>(queue_info.borrow_unchecked_mut())? };
    let queue_pda = TransferQueue::create_address(&mint_info.address(), &[queue.bump])?;
    if &queue_pda != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let now = Clock::get()?.unix_timestamp;
    let (processed_transfers, transfers) = queue.processed_transfers(now);

    if processed_transfers == 0 {
        return Ok(());
    }

    let actual_transfers = &transfers[..processed_transfers];

    let bump = [queue.bump];
    let seed = TransferQueue::signer_seeds(&mint_info.address(), &bump);
    let signer = Signer::from(&seed);

    let decimals = {
        if !mint_info.owned_by(token_program_info.address()) {
            return Err(ProgramError::IllegalOwner);
        }
        let mint_data = unsafe { mint_info.borrow_unchecked() };
        if mint_data.len() < Mint::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
        mint.decimals()
    };

    if !shuttle_eata_info.owned_by(&crate::ID) {
        return Err(ProgramError::IllegalOwner);
    }
    let shuttle_eata =
        unsafe { load_unchecked::<EphemeralAta>(shuttle_eata_info.borrow_unchecked())? };
    let shuttle_eata_pda = Address::create_program_address(
        &[
            shuttle_info.address().as_ref(),
            mint_info.address().as_ref(),
            args.shuttle_eata_bump.to_le_bytes().as_ref(),
        ],
        &ephemeral_spl_api::program::ID,
    )?;
    if &shuttle_eata_pda != shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if shuttle_eata.owner != *shuttle_info.address() {
        return Err(ProgramError::IllegalOwner);
    }

    // Transfer tokens to the
    TransferChecked {
        mint: mint_info,
        from: queue_ata_info,
        to: shuttle_ata_info,
        authority: queue_info,
        amount: actual_transfers.iter().map(|(amount, _)| amount).sum(),
        decimals,
        token_program: token_program_info.address(),
    }
    .invoke_signed(&[signer.clone()])?;

    // Cancel the shuttle crank if it exists
    if let Some(crank_id) = queue.shuttle_crank_id {
        CancelCrankCpi {
            authority: queue_info.clone(),
            magic_program: magic_program_info.clone(),
            crank_id,
        }
        .invoke_signed(&[signer.clone()])?;
        queue.shuttle_crank_id = None;
    }

    // TODO(dode): Use actions to schedule mainnet transfers
    // TODO(dode): overfund the shuttle PDA to pay for undelegations
    // ephemeral_rollups_pinocchio::instruction::commit_and_undelegate_accounts(
    //     shuttle_info,
    //     &[shuttle_ata_info.clone()],
    //     magic_context_info,
    //     magic_program_info,
    //     Some(&[signer]),
    // )?;

    Ok(())
}

#[repr(C)]
pub struct ProcessTransfersArgs {
    shuttle_eata_bump: u8,
}

impl ProcessTransfersArgs {
    pub fn try_from_bytes(bytes: &[u8]) -> Result<&ProcessTransfersArgs, ProgramError> {
        if bytes.len() != 1 {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(unsafe { &*(bytes.as_ptr() as *const ProcessTransfersArgs) })
    }
}
