use pinocchio::{cpi::Seed, error::ProgramError, Address};

use crate::state::load_mut;

use super::{load, load_initialized, Initializable, RawType};

const LEGACY_EPHEMERAL_ATA_LEN: usize = 72;

/// Internal representation of a token account data.
#[repr(C)]
pub struct EphemeralAta {
    /// The owner of the eata
    pub owner: Address,
    /// The mint associated with this account
    pub mint: Address,
    /// The amount of tokens this account holds.
    pub amount: u64,
    /// The bump of the eata
    pub bump: u8,
    pub _pad0: [u8; 7],
}

impl RawType for EphemeralAta {
    const LEN: usize = core::mem::size_of::<EphemeralAta>();
}

impl Initializable for EphemeralAta {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.mint != Address::default()
    }
}

impl EphemeralAta {
    #[inline(always)]
    pub fn derive_pda(
        owner: &Address,
        mint: &Address,
        bump_seed: u8,
    ) -> Result<Address, ProgramError> {
        let bump_seed = [bump_seed];
        let pda = Address::create_program_address(
            &Self::seeds_with_bump(owner, mint, &bump_seed),
            &crate::ID,
        )?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn find_pda(owner: &Address, mint: &Address) -> (Address, u8) {
        Address::find_program_address(&Self::seeds(owner, mint), &crate::ID)
    }

    #[inline(always)]
    pub fn seeds<'a>(owner: &'a Address, mint: &'a Address) -> [&'a [u8]; 2] {
        [owner.as_ref(), mint.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(
        owner: &'a Address,
        mint: &'a Address,
        bump: &'a [u8],
    ) -> [&'a [u8]; 3] {
        [owner.as_ref(), mint.as_ref(), bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(
        owner: &'a Address,
        mint: &'a Address,
        bump: &'a [u8],
    ) -> [Seed<'a>; 3] {
        [
            Seed::from(owner.as_ref()),
            Seed::from(mint.as_ref()),
            Seed::from(bump),
        ]
    }
}

// Temporary compatibility shim for legacy 72-byte EphemeralAta accounts.
// Some undelegation flows can still restore old snapshots without the stored bump.
// Keep it self-contained here so it can be removed cleanly once those snapshots are gone.
#[repr(C)]
struct LegacyEphemeralAta {
    owner: Address,
    mint: Address,
    amount: u64,
}

impl RawType for LegacyEphemeralAta {
    const LEN: usize = LEGACY_EPHEMERAL_ATA_LEN;
}

enum EphemeralAtaCompatMutInner<'a> {
    Current(&'a mut EphemeralAta),
    Legacy(&'a mut LegacyEphemeralAta),
}

pub struct EphemeralAtaCompatMut<'a>(EphemeralAtaCompatMutInner<'a>);

impl EphemeralAtaCompatMut<'_> {
    #[inline(always)]
    pub fn owner(&self) -> &Address {
        match &self.0 {
            EphemeralAtaCompatMutInner::Current(ephemeral_ata) => &ephemeral_ata.owner,
            EphemeralAtaCompatMutInner::Legacy(ephemeral_ata) => &ephemeral_ata.owner,
        }
    }

    #[inline(always)]
    pub fn mint(&self) -> &Address {
        match &self.0 {
            EphemeralAtaCompatMutInner::Current(ephemeral_ata) => &ephemeral_ata.mint,
            EphemeralAtaCompatMutInner::Legacy(ephemeral_ata) => &ephemeral_ata.mint,
        }
    }

    #[inline(always)]
    pub fn amount(&self) -> u64 {
        match &self.0 {
            EphemeralAtaCompatMutInner::Current(ephemeral_ata) => ephemeral_ata.amount,
            EphemeralAtaCompatMutInner::Legacy(ephemeral_ata) => ephemeral_ata.amount,
        }
    }

    #[inline(always)]
    pub fn set_amount(&mut self, amount: u64) {
        match &mut self.0 {
            EphemeralAtaCompatMutInner::Current(ephemeral_ata) => ephemeral_ata.amount = amount,
            EphemeralAtaCompatMutInner::Legacy(ephemeral_ata) => ephemeral_ata.amount = amount,
        }
    }
}

#[inline(always)]
pub fn read_ephemeral_ata_compat(
    bytes: &[u8],
) -> Result<(Address, Address, u64, u8), ProgramError> {
    if bytes.len() == EphemeralAta::LEN {
        let ephemeral_ata = load_initialized::<EphemeralAta>(bytes)?;
        #[allow(clippy::clone_on_copy)]
        let owner = ephemeral_ata.owner.clone();
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        return Ok((owner, mint, ephemeral_ata.amount, ephemeral_ata.bump));
    }

    if bytes.len() == LEGACY_EPHEMERAL_ATA_LEN {
        let ephemeral_ata = load::<LegacyEphemeralAta>(bytes)?;
        if ephemeral_ata.mint == Address::default() {
            return Err(ProgramError::UninitializedAccount);
        }
        #[allow(clippy::clone_on_copy)]
        let owner = ephemeral_ata.owner.clone();
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        return Ok((
            owner,
            mint,
            ephemeral_ata.amount,
            EphemeralAta::find_pda(&owner, &mint).1,
        ));
    }

    Err(ProgramError::InvalidAccountData)
}

#[inline(always)]
pub fn load_ephemeral_ata_compat_mut(
    bytes: &mut [u8],
) -> Result<EphemeralAtaCompatMut<'_>, ProgramError> {
    if bytes.len() == EphemeralAta::LEN {
        return Ok(EphemeralAtaCompatMut(EphemeralAtaCompatMutInner::Current(
            load_mut::<EphemeralAta>(bytes)?,
        )));
    }

    if bytes.len() == LEGACY_EPHEMERAL_ATA_LEN {
        return Ok(EphemeralAtaCompatMut(EphemeralAtaCompatMutInner::Legacy(
            load_mut::<LegacyEphemeralAta>(bytes)?,
        )));
    }

    Err(ProgramError::InvalidAccountData)
}
