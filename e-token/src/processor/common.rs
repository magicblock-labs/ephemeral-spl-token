use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_mut_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    RawType,
};
use pinocchio::{
    cpi::{Seed, Signer},
    sysvars::rent::Rent,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Assign, CreateAccount, Transfer};

pub fn create_pda(
    rent_sysvar: &Rent,
    space: usize,
    final_space: usize,
    payer: &AccountView,
    account: &AccountView,
    owner: &Address,
    signers: &[Signer],
) -> ProgramResult {
    let lamports = rent_sysvar.try_minimum_balance(final_space)?;

    if account.lamports() == 0 {
        // Create the account if it does not exist.
        CreateAccount {
            from: payer,
            to: account,
            lamports,
            space: space as u64,
            owner,
        }
        .invoke_signed(signers)
    } else {
        let required_lamports = lamports.saturating_sub(account.lamports());

        // Transfer lamports from `payer` to `account` if needed.
        if required_lamports > 0 {
            Transfer {
                from: payer,
                to: account,
                lamports: required_lamports,
            }
            .invoke_signed(signers)?;
        }

        // Assign the account to the specified owner.
        Assign { account, owner }.invoke_signed(signers)?;

        // Allocate the required space for the account.
        //
        // SAFETY: There are no active borrows of the `account`.
        // This was checked by the `Assign` CPI above.
        unsafe { account.resize_unchecked(space) }
    }
}

pub fn initialize_ephemeral_ata(
    rent_sysvar: &Rent,
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

    create_pda(
        rent_sysvar,
        EphemeralAta::LEN,
        EphemeralAta::LEN,
        payer_info,
        ephemeral_ata_info,
        &ephemeral_spl_api::program::id_address(),
        &[signer_seeds],
    )?;

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
    rent_sysvar: &Rent,
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

    create_pda(
        rent_sysvar,
        ShuttleEphemeralAta::LEN,
        ShuttleEphemeralAta::LEN,
        payer_info,
        shuttle_info,
        &ephemeral_spl_api::program::id_address(),
        &[signer_seeds],
    )?;

    let shuttle =
        unsafe { load_mut_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked_mut())? };

    shuttle.owner = owner_info.address().clone();
    shuttle.payer = payer_info.address().clone();
    shuttle.id = shuttle_id;

    Ok(())
}
