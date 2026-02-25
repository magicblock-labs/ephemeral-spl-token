use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_mut_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    RawType,
};
use pinocchio::{
    cpi::{Seed, Signer},
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;

pub fn initialize_ephemeral_ata(
    payer_info: &AccountView,
    user_info: &AccountView,
    mint_info: &AccountView,
    ephemeral_ata_info: &AccountView,
    eata_bump: u8,
) -> ProgramResult {
    let bump = [eata_bump];
    let seed = [
        Seed::from(user_info.address().as_ref()),
        Seed::from(mint_info.address().as_ref()),
        Seed::from(&bump),
    ];
    let signer_seeds = Signer::from(&seed);

    CreateAccount {
        from: payer_info,
        to: ephemeral_ata_info,
        space: EphemeralAta::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?,
        owner: &ephemeral_spl_api::program::id_address(),
    }
    .invoke_signed(&[signer_seeds])?;

    // Ensure account data has the expected size
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };

    // Initialize the ephemeral ATA
    // Set the owner to the provided user; payer only funds account creation
    ephemeral_ata.owner = user_info.address().clone();
    ephemeral_ata.mint = mint_info.address().clone();
    ephemeral_ata.amount = 0;

    Ok(())
}

pub fn initialize_shuttle(
    payer_info: &AccountView,
    owner_info: &AccountView,
    mint_info: &AccountView,
    shuttle_info: &AccountView,
    shuttle_id: u32,
    shuttle_bump: &[u8],
) -> ProgramResult {
    let shuttle_id_seed = shuttle_id.to_le_bytes();
    let seed = [
        Seed::from(owner_info.address().as_ref()),
        Seed::from(mint_info.address().as_ref()),
        Seed::from(shuttle_id_seed.as_ref()),
        Seed::from(shuttle_bump),
    ];
    let signer_seeds = Signer::from(&seed);

    CreateAccount {
        from: payer_info,
        to: shuttle_info,
        space: ShuttleEphemeralAta::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(ShuttleEphemeralAta::LEN)?,
        owner: &ephemeral_spl_api::program::id_address(),
    }
    .invoke_signed(&[signer_seeds])?;

    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked_mut())? };

    shuttle.owner = owner_info.address().clone();
    shuttle.payer = payer_info.address().clone();
    shuttle.id = shuttle_id;

    Ok(())
}
