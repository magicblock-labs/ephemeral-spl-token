use pinocchio::error::ProgramError;

pub mod ephemeral_ata;
pub mod global_vault;
pub mod group_receipt;
pub mod shuttle_ephemeral_ata;
pub mod stash;
pub mod transfer_queue;
pub mod transfer_queue_refill;

/// Trait to represent a type that can be initialized.
pub trait Initializable {
    /// Return `true` if the object is initialized.
    fn is_initialized(&self) -> bool;
}

/// Return a reference for an initialized `T` from the given bytes.
///
/// # Safety
///
/// The caller must ensure that `bytes` is properly aligned for `T` and contains
/// a valid representation of `T`. The length and initialization checks below do
/// not prove either property.
#[inline(always)]
pub fn load_initialized<T: Initializable + RawType>(bytes: &[u8]) -> Result<&T, ProgramError> {
    load(bytes).and_then(|t: &T| {
        // checks if the data is initialized
        if t.is_initialized() {
            Ok(t)
        } else {
            Err(ProgramError::UninitializedAccount)
        }
    })
}

/// Return a `T` reference from the given bytes.
///
/// This function does not check if the data is initialized.
///
/// # Safety
///
/// The caller must ensure that `bytes` is properly aligned for `T` and contains
/// a valid representation of `T`. The length check below does not prove either
/// property.
#[inline(always)]
pub fn load<T: RawType>(bytes: &[u8]) -> Result<&T, ProgramError> {
    if bytes.len() != T::LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(unsafe { &*(bytes.as_ptr() as *const T) })
}

/// Return a mutable reference for an initialized `T` from the given bytes.
///
/// # Safety
///
/// The caller must ensure that `bytes` is properly aligned for `T` and contains
/// a valid representation of `T`. The length and initialization checks below do
/// not prove either property.
#[inline(always)]
pub fn load_mut_initialized<T: Initializable + RawType>(
    bytes: &mut [u8],
) -> Result<&mut T, ProgramError> {
    load_mut(bytes).and_then(|t: &mut T| {
        // checks if the data is initialized
        if t.is_initialized() {
            Ok(t)
        } else {
            Err(ProgramError::UninitializedAccount)
        }
    })
}

/// Return a mutable `T` reference from the given bytes.
///
/// This function does not check if the data is initialized.
///
/// # Safety
///
/// The caller must ensure that `bytes` is properly aligned for `T` and contains
/// a valid representation of `T`. The length check below does not prove either
/// property.
#[inline(always)]
pub fn load_mut<T: RawType>(bytes: &mut [u8]) -> Result<&mut T, ProgramError> {
    if bytes.len() != T::LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(unsafe { &mut *(bytes.as_mut_ptr() as *mut T) })
}

/// Type alias for fields represented as `COption`.
pub type COption<T> = ([u8; 4], T);

/// Marker trait for types that can cast from a raw pointer.
///
/// It is up to the type implementing this trait to guarantee that the cast is safe,
/// i.e., that the fields of the type are well aligned and there are no padding bytes.
pub trait RawType {
    /// The length of the type.
    ///
    /// This must be equal to the size of each individual field in the type.
    const LEN: usize;
}
