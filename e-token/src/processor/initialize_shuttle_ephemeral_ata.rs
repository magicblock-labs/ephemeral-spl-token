use crate::processor::common::initialize_shuttle;
use crate::processor::initialize_ephemeral_ata::process_initialize_ephemeral_ata;
use core::marker::PhantomData;
use ephemeral_spl_api::state::{load_unchecked, Initializable};
use pinocchio::sysvars::{rent::Rent, Sysvar};
use {
    ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleEphemeralAta,
    pinocchio::{error::ProgramError, AccountView, ProgramResult},
};

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

    let shuttle_id_seed = args.shuttle_id().to_le_bytes();
    let bump = [args.bump()];
    let derived_shuttle_pda = ephemeral_spl_api::Address::create_program_address(
        &[
            owner_info.address().as_ref(),
            mint_info.address().as_ref(),
            shuttle_id_seed.as_ref(),
            bump.as_ref(),
        ],
        &ephemeral_spl_api::program::ID,
    )
    .map_err(|_| ProgramError::InvalidSeeds)?;
    if derived_shuttle_pda != *shuttle_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let shuttle_is_owned_by_program =
        unsafe { shuttle_info.owner().eq(&ephemeral_spl_api::program::ID) };

    if !shuttle_is_owned_by_program {
        let bump = [args.bump()];
        initialize_shuttle(
            &Rent::get()?,
            payer_info,
            owner_info,
            mint_info,
            shuttle_info,
            args.shuttle_id(),
            &bump,
        )?;
    } else {
        let shuttle =
            unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
        if !shuttle.is_initialized()
            || shuttle.id != args.shuttle_id()
            || shuttle.owner != *owner_info.address()
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    let (derived_shuttle_eata, shuttle_eata_bump) =
        ephemeral_spl_api::Address::find_program_address(
            &[
                shuttle_info.address().as_ref(),
                mint_info.address().as_ref(),
            ],
            &ephemeral_spl_api::program::ID,
        );
    if derived_shuttle_eata != *shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let eata_init_data = [shuttle_eata_bump];
    let eata_init_accounts = [
        shuttle_eata_info.clone(),
        payer_info.clone(),
        shuttle_info.clone(),
        mint_info.clone(),
    ];
    process_initialize_ephemeral_ata(&eata_init_accounts, &eata_init_data)?;

    pinocchio_associated_token_account::instructions::CreateIdempotent {
        funding_account: payer_info,
        account: shuttle_wallet_ata_info,
        wallet: shuttle_info,
        mint: mint_info,
        system_program: system_program_info,
        token_program: token_program_info,
    }
    .invoke()?;

    Ok(())
}

pub struct InitializeShuttleEphemeralAta<'a> {
    raw: *const u8,
    _data: PhantomData<&'a [u8]>,
}

impl InitializeShuttleEphemeralAta<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<InitializeShuttleEphemeralAta, ProgramError> {
        if bytes.len() < 5 {
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

    #[inline]
    pub fn bump(&self) -> u8 {
        unsafe { *self.raw.add(4) }
    }
}
