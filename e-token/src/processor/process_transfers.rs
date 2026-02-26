use ephemeral_spl_api::state::transfer_queue::TransferQueue;
use pinocchio::{
    cpi::Signer,
    sysvars::{clock::Clock, Sysvar},
};
use pinocchio_token_2022::{instructions::TransferChecked, state::Mint};

use {
    ephemeral_spl_api::state::load_mut_unchecked,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

#[inline(always)]
pub fn process_transfers(accounts: &[AccountView], _instruction_data: &[u8]) -> ProgramResult {
    let [
        _payer_info, // writable, signer
        mint_info, // readonly
        queue_info, // writable, PDA [mint]
        queue_ata_info, // writable, ATA for [depositor, mint]
        _shuttle_info, // writable, shuttle PDA
        shuttle_ata_info, // writable, ATA for [shuttle, mint]
        _shuttle_eata_info, // readonly, EATA for [shuttle, mint]
        token_program_info, // []
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let queue = unsafe { load_mut_unchecked::<TransferQueue>(queue_info.borrow_unchecked_mut())? };
    let queue_pda = TransferQueue::create_address(&mint_info.address(), &[queue.bump])?;
    if &queue_pda != queue_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    pinocchio_log::log!("Now: {}", Clock::get()?.unix_timestamp);
    let (processed_transfers, transfers) = queue.processed_transfers()?;
    let actual_transfers = &transfers[..processed_transfers];

    pinocchio_log::log!("Processed transfers: {}", processed_transfers);
    for (amount, destination) in actual_transfers {
        pinocchio_log::log!(
            "Amount: {}, Destination: {}",
            *amount,
            &destination.as_ref()[..]
        );
    }

    let bump = [queue.bump];
    let seed = TransferQueue::signer_seeds(&mint_info.address(), &bump);
    let signer = Signer::from(&seed);

    let decimals = {
        let mint_data = unsafe { mint_info.borrow_unchecked() };
        if mint_data.len() < Mint::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
        mint.decimals()
    };

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
    .invoke_signed(&[signer])?;

    // TODO(dode): Use actions to schedule mainnet transfers
    // undelegate(&shuttle_eata_info, &crate::ID, buffer, payer, callback_args)?;

    Ok(())
}
