use ephemeral_spl_api::{
    require, require_eq_keys,
    state::{
        ephemeral_ata::EphemeralAta, load_initialized, load_mut,
        shuttle_ephemeral_ata::ShuttleMetadata, RawType,
    },
};
use pinocchio::{
    cpi::Signer,
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_system::instructions::{CreateAccount, Transfer};

const EPHEMERAL_ATA_V0_LEN: usize = 72;
const SHUTTLE_METADATA_V0_LEN: usize = 68;

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
    require_eq_keys!(
        &derived_pda,
        ephemeral_ata_info.address(),
        ProgramError::InvalidSeeds
    );

    // Make init idempotent even if the account is currently delegated (owner changed).
    if let Ok(ephemeral_ata) =
        load_initialized::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked() })
    {
        if ephemeral_ata.owner == *user_info.address() && ephemeral_ata.mint == *mint_info.address()
        {
            return Ok(());
        }
    }

    // Migrate legacy ephemeral ATAs
    // TODO: Remove this migration path once all deployed ATAs are upgraded.
    if ephemeral_ata_info.data_len() == EPHEMERAL_ATA_V0_LEN
        && ephemeral_ata_info.owned_by(&crate::ID)
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

        let ephemeral_ata =
            load_mut::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;

        // Set the missing bump
        ephemeral_ata.bump = eata_bump;

        return Ok(());
    }

    // Any other pre-existing account at this PDA is invalid for initialization.
    require!(
        ephemeral_ata_info.lamports() == 0,
        ProgramError::InvalidAccountData
    );

    let bump = [eata_bump];
    let seed = EphemeralAta::signer_seeds(user_info.address(), mint_info.address(), &bump);
    let signer_seeds = Signer::from(&seed);

    let create_ephemeral_ata = CreateAccount {
        from: sponsor_info,
        to: ephemeral_ata_info,
        space: EphemeralAta::LEN as u64,
        lamports: Rent::get()?.try_minimum_balance(EphemeralAta::LEN)?,
        owner: &crate::ID,
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
        load_mut::<EphemeralAta>(unsafe { ephemeral_ata_info.borrow_unchecked_mut() })?;

    // Initialize the ephemeral ATA
    // Set the owner to the provided user; payer only funds account creation
    ephemeral_ata.owner = *user_info.address();
    ephemeral_ata.mint = *mint_info.address();
    ephemeral_ata.amount = 0;
    ephemeral_ata.bump = eata_bump;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(crate) fn initialize_shuttle_ephemeral_ata_with_sponsor(
    sponsor_info: &AccountView,
    sponsor_signer: Option<Signer<'_, '_>>,
    shuttle_info: &AccountView,
    shuttle_eata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    refund_recipient_info: &AccountView,
    owner_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    system_program_info: &AccountView,
    shuttle_id: u32,
) -> ProgramResult {
    let shuttle_id_seed = shuttle_id.to_le_bytes();
    let (derived_shuttle_pda, shuttle_bump) =
        ShuttleMetadata::find_pda(owner_info.address(), mint_info.address(), shuttle_id);
    require_eq_keys!(
        &derived_shuttle_pda,
        shuttle_info.address(),
        ProgramError::InvalidSeeds
    );

    let shuttle_is_owned_by_program = unsafe { shuttle_info.owner().eq(&crate::ID) };

    if !shuttle_is_owned_by_program {
        let bump = [shuttle_bump];
        let shuttle_seed = ShuttleMetadata::signer_seeds(
            owner_info.address(),
            mint_info.address(),
            &shuttle_id_seed,
            &bump,
        );
        let shuttle_signer = Signer::from(&shuttle_seed);

        let create_shuttle = CreateAccount {
            from: sponsor_info,
            to: shuttle_info,
            space: ShuttleMetadata::LEN as u64,
            lamports: Rent::get()?.try_minimum_balance(ShuttleMetadata::LEN)?,
            owner: &crate::ID,
        };
        if let Some(sponsor_signer) = sponsor_signer.as_ref() {
            let signers = [sponsor_signer.clone(), shuttle_signer];
            create_shuttle.invoke_signed(&signers)?;
        } else {
            let signers = [shuttle_signer];
            create_shuttle.invoke_signed(&signers)?;
        }

        let shuttle = unsafe { load_mut::<ShuttleMetadata>(shuttle_info.borrow_unchecked_mut())? };

        shuttle.owner = *owner_info.address();
        shuttle.payer = *refund_recipient_info.address();
        shuttle.id = shuttle_id;
        shuttle.bump = shuttle_bump;
    } else {
        // Migrate legacy shuttle metadata
        if shuttle_info.data_len() == SHUTTLE_METADATA_V0_LEN {
            let current_lamports = shuttle_info.lamports();
            if current_lamports < Rent::get()?.try_minimum_balance(ShuttleMetadata::LEN)? {
                if let Some(sponsor_signer) = sponsor_signer.clone() {
                    Transfer {
                        from: sponsor_info,
                        to: shuttle_info,
                        lamports: Rent::get()?.try_minimum_balance(ShuttleMetadata::LEN)?
                            - current_lamports,
                    }
                    .invoke_signed(&[sponsor_signer])?;
                } else {
                    Transfer {
                        from: sponsor_info,
                        to: shuttle_info,
                        lamports: Rent::get()?.try_minimum_balance(ShuttleMetadata::LEN)?
                            - current_lamports,
                    }
                    .invoke()?;
                }
            }
            shuttle_info.resize(ShuttleMetadata::LEN)?;
            let shuttle =
                load_mut::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked_mut() })?;
            shuttle.bump = shuttle_bump;
        }

        let shuttle =
            load_initialized::<ShuttleMetadata>(unsafe { shuttle_info.borrow_unchecked() })?;
        require!(
            shuttle.id == shuttle_id
                && shuttle.owner == *owner_info.address()
                && shuttle.payer == *refund_recipient_info.address(),
            ProgramError::InvalidAccountData
        );
    }

    initialize_ephemeral_ata_with_sponsor(
        shuttle_eata_info,
        sponsor_info,
        sponsor_signer.clone(),
        shuttle_info,
        mint_info,
    )?;

    let ata_ix = pinocchio_associated_token_account::instructions::CreateIdempotent {
        funding_account: sponsor_info,
        account: shuttle_wallet_ata_info,
        wallet: shuttle_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    };
    if let Some(sponsor_signer) = sponsor_signer {
        ata_ix.invoke_signed(&[sponsor_signer])?;
    } else {
        ata_ix.invoke()?;
    }

    Ok(())
}
