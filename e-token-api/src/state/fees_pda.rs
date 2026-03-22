use pinocchio::{cpi::Seed, error::ProgramError, Address};

use super::{Initializable, RawType};

/// Four-byte account discriminator, reused as the PDA seed prefix for
/// validator-scoped fee delegation accounts.
pub const FEES_PDA_TAG: [u8; 4] = *b"FEES";
pub const FEES_PDA_SEED: &[u8] = &FEES_PDA_TAG;

/// Validator-scoped PDA that anchors fee delegation and commit flows.
///
/// The account does not track fee amounts. It only stores the validator and bump
/// for the derived `["FEES", validator]` address so the program can validate the
/// account and sign for delegation/commit operations.
#[repr(C)]
pub struct FeesPda {
    pub tag: [u8; 4],
    pub validator: Address,
    pub bump: u8,
}

impl RawType for FeesPda {
    const LEN: usize = core::mem::size_of::<FeesPda>();
}

impl Initializable for FeesPda {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.tag == FEES_PDA_TAG
    }
}

impl FeesPda {
    #[inline(always)]
    pub fn create_pda(validator: &Address, bump_seed: u8) -> Result<Address, ProgramError> {
        let bump = [bump_seed];
        let pda = Address::create_program_address(
            &[FEES_PDA_SEED, validator.as_ref(), &bump],
            &crate::ID,
        )?;
        Ok(pda)
    }

    #[inline(always)]
    pub fn find_pda(validator: &Address) -> (Address, u8) {
        Address::find_program_address(&[FEES_PDA_SEED, validator.as_ref()], &crate::ID)
    }

    #[inline(always)]
    pub fn seeds<'a>(validator: &'a Address) -> [&'a [u8]; 2] {
        [&FEES_PDA_SEED, validator.as_ref()]
    }

    #[inline(always)]
    pub fn seeds_with_bump<'a>(validator: &'a Address, bump: &'a [u8]) -> [&'a [u8]; 3] {
        [&FEES_PDA_SEED, validator.as_ref(), &bump]
    }

    #[inline(always)]
    pub fn signer_seeds<'a>(validator: &'a Address, bump: &'a [u8]) -> [Seed<'a>; 3] {
        [
            Seed::from(FEES_PDA_SEED),
            Seed::from(validator.as_ref()),
            Seed::from(bump),
        ]
    }
}
