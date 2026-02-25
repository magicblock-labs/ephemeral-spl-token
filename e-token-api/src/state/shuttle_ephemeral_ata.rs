use pinocchio::Address;

use super::{Initializable, RawType};

/// Internal representation of a shuttle ephemeral token account.
#[repr(C)]
pub struct ShuttleEphemeralAta {
    /// The logical owner of this shuttle account (seed used for shuttle PDA).
    pub owner: Address,
    /// The account that funded rent for this shuttle account.
    pub payer: Address,
    /// User-defined identifier to allow multiple shuttles per [owner, mint].
    pub id: u32,
}

impl RawType for ShuttleEphemeralAta {
    const LEN: usize = core::mem::size_of::<ShuttleEphemeralAta>();
}

impl Initializable for ShuttleEphemeralAta {
    #[inline(always)]
    fn is_initialized(&self) -> bool {
        self.owner != Address::default()
    }
}

impl ShuttleEphemeralAta {
    #[inline(always)]
    pub fn find_pda(owner: &Address, mint: &Address, shuttle_id: u32) -> (Address, u8) {
        Address::find_program_address(
            &[
                owner.as_ref(),
                mint.as_ref(),
                shuttle_id.to_le_bytes().as_ref(),
            ],
            &crate::program::id_address(),
        )
    }
}
