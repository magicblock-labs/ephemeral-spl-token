use ephemeral_spl_api::state::{Initializable, RawType};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::{CreateAccount, Transfer};
use {
    ephemeral_spl_api::state::ephemeral_ata::EphemeralAta,
    ephemeral_spl_api::state::{load_mut_unchecked, load_unchecked},
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

const EPHEMERAL_ATA_V0_LEN: usize = 72;

#[inline(always)]
pub fn process_initialize_ephemeral_ata(
    accounts: &[AccountView],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [writable] Ephemeral ATA account (PDA derived from [user, mint])
    // 1. []         Payer (funding account)
    // 2. []         User  (seed)
    // 3. []         Mint  (seed)

    let [ephemeral_ata_info, payer_info, user_info, mint_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    initialize_ephemeral_ata_with_sponsor(
        ephemeral_ata_info,
        payer_info,
        None,
        user_info,
        mint_info,
    )
}

#[inline(never)]
pub(crate) fn initialize_ephemeral_ata_with_sponsor(
    ephemeral_ata_info: &AccountView,
    sponsor_info: &AccountView,
    sponsor_signer: Option<Signer<'_, '_>>,
    user_info: &AccountView,
    mint_info: &AccountView,
) -> ProgramResult {
    // Validate PDA derivation up front, even for idempotent re-initialization.
    let (derived_pda, eata_bump) = EphemeralAta::find_pda(user_info.address(), mint_info.address());
    if derived_pda != *ephemeral_ata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    // Make init idempotent even if the account is currently delegated (owner changed).
    if let Ok(ephemeral_ata) =
        unsafe { load_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked()) }
    {
        if ephemeral_ata.is_initialized()
            && ephemeral_ata.owner == *user_info.address()
            && ephemeral_ata.mint == *mint_info.address()
        {
            return Ok(());
        }
    }

    // Migrate legacy ephemeral ATAs
    // TODO: Remove this migration path once all deployed ATAs are upgraded.
    if ephemeral_ata_info.data_len() == EPHEMERAL_ATA_V0_LEN
        && ephemeral_ata_info.owned_by(&crate::ID.into())
    {
        let current_lamports = ephemeral_ata_info.lamports();
        if current_lamports < Rent::get()?.try_minimum_balance(EphemeralAta::LEN)? {
            if let Some(sponsor_signer) = sponsor_signer {
                Transfer {
                    from: sponsor_info,
                    to: ephemeral_ata_info,
                    lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?
                        - current_lamports,
                }
                .invoke_signed(&[sponsor_signer])?;
            } else {
                Transfer {
                    from: sponsor_info,
                    to: ephemeral_ata_info,
                    lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?
                        - current_lamports,
                }
                .invoke()?;
            }
        }

        ephemeral_ata_info.resize(EphemeralAta::LEN)?;

        let ephemeral_ata = unsafe {
            load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())?
        };

        // Set the missing bump
        ephemeral_ata.bump = eata_bump;

        return Ok(());
    }

    // Any other pre-existing account at this PDA is invalid for initialization.
    if ephemeral_ata_info.lamports() > 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    let bump = [eata_bump];
    let seed = EphemeralAta::signer_seeds(user_info.address(), mint_info.address(), &bump);
    let signer_seeds = Signer::from(&seed);

    let create_ephemeral_ata = CreateAccount {
        from: sponsor_info,
        to: ephemeral_ata_info,
        space: EphemeralAta::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?,
        owner: &ephemeral_spl_api::ID,
    };
    if let Some(sponsor_signer) = sponsor_signer {
        let signers = [sponsor_signer, signer_seeds];
        create_ephemeral_ata.invoke_signed(&signers)?;
    } else {
        let signers = [signer_seeds];
        create_ephemeral_ata.invoke_signed(&signers)?;
    }

    // Ensure account data has the expected size
    let ephemeral_ata =
        unsafe { load_mut_unchecked::<EphemeralAta>(ephemeral_ata_info.borrow_unchecked_mut())? };

    // Initialize the ephemeral ATA
    // Set the owner to the provided user; payer only funds account creation
    ephemeral_ata.owner = *user_info.address();
    ephemeral_ata.mint = *mint_info.address();
    ephemeral_ata.amount = 0;
    ephemeral_ata.bump = eata_bump;

    Ok(())
}
