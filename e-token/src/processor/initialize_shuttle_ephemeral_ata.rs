use crate::processor::initialize_ephemeral_ata::initialize_ephemeral_ata_with_sponsor;
use core::marker::PhantomData;
use ephemeral_spl_api::state::{load_mut_unchecked, load_unchecked, Initializable, RawType};
use pinocchio::cpi::Signer;
use pinocchio::sysvars::rent::Rent;
use pinocchio::sysvars::Sysvar;
use pinocchio_system::instructions::{CreateAccount, Transfer};
use {
    ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

const SHUTTLE_METADATA_V0_LEN: usize = 68;

#[inline(always)]
pub fn process_initialize_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    // Expected accounts:
    // 0. [signer]   Payer (funding account)
    // 1. [writable] Shuttle metadata account (PDA derived from [owner, mint, shuttle_id])
    // 2. [writable] Shuttle EATA account (PDA derived from [shuttle_metadata, mint])
    // 3. [writable] Shuttle wallet ATA account (ATA for [shuttle_metadata, mint])
    // 4. []         Owner (seed)
    // 5. []         Mint  (seed)
    // 6. []         Token program
    // 7. []         Associated token program
    // 8. []         System program
    let args = InitializeShuttleEphemeralAta::try_from_bytes(instruction_data)?;

    let [payer_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, owner_info, mint_info, token_program_info, _associated_token_program_info, system_program_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    initialize_shuttle_ephemeral_ata_with_sponsor(
        payer_info,
        None,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        payer_info,
        owner_info,
        mint_info,
        token_program_info,
        system_program_info,
        args.shuttle_id(),
    )?;

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
    if derived_shuttle_pda != *shuttle_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let shuttle_is_owned_by_program = unsafe { shuttle_info.owner().eq(&ephemeral_spl_api::ID) };

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
            owner: &ephemeral_spl_api::ID,
        };
        if let Some(sponsor_signer) = sponsor_signer.as_ref() {
            let signers = [sponsor_signer.clone(), shuttle_signer];
            create_shuttle.invoke_signed(&signers)?;
        } else {
            let signers = [shuttle_signer];
            create_shuttle.invoke_signed(&signers)?;
        }

        let shuttle =
            unsafe { load_mut_unchecked::<ShuttleMetadata>(shuttle_info.borrow_unchecked_mut())? };

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
            let shuttle = unsafe {
                load_mut_unchecked::<ShuttleMetadata>(shuttle_info.borrow_unchecked_mut())?
            };
            shuttle.bump = shuttle_bump;
        }

        let shuttle =
            unsafe { load_unchecked::<ShuttleMetadata>(shuttle_info.borrow_unchecked())? };
        if !shuttle.is_initialized()
            || shuttle.id != shuttle_id
            || shuttle.owner != *owner_info.address()
            || shuttle.payer != *refund_recipient_info.address()
        {
            return Err(ProgramError::InvalidAccountData);
        }
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

pub struct InitializeShuttleEphemeralAta<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeShuttleEphemeralAta<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeShuttleEphemeralAta<'_>, ProgramError> {
        if bytes.len() < 4 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(InitializeShuttleEphemeralAta {
            raw: bytes.as_ptr(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn shuttle_id(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }
}
