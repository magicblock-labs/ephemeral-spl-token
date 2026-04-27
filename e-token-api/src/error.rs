use pinocchio::error::{ProgramError, ToStr};

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EphemeralSplError {
    // invalid discriminator
    InstructionNotFound = 1,
    // account already initialized / in use
    AlreadyInUse = 2,
    // Ephemeral ATA mismatch
    EphemeralAtaMismatch = 3,
    // mint account mismatch
    MintMismatch = 4,
    // vault token account mismatch
    VaultTokenAccountMismatch = 5,
    // too many accounts were passed to the instruction
    TooManyAccountKeys = 6,
    // internal invariant / unreachable conversion failure
    InfallibleError = 255,
}

impl From<EphemeralSplError> for ProgramError {
    fn from(e: EphemeralSplError) -> Self {
        Self::Custom(e as u32)
    }
}

impl ToStr for EphemeralSplError {
    fn to_str(&self) -> &'static str {
        use EphemeralSplError::*;

        match self {
            InstructionNotFound => "EphemeralSplError::InstructionNotFound",
            AlreadyInUse => "EphemeralSplError::AlreadyInUse",
            EphemeralAtaMismatch => "EphemeralSplError::EphemeralAtaMismatch",
            MintMismatch => "EphemeralSplError::MintMismatch",
            VaultTokenAccountMismatch => "EphemeralSplError::VaultTokenAccountMismatch",
            TooManyAccountKeys => "EphemeralSplError::TooManyAccountKeys",
            InfallibleError => "EphemeralSplError::InfallibleError",
        }
    }
}

impl core::convert::TryFrom<u32> for EphemeralSplError {
    type Error = ProgramError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(EphemeralSplError::InstructionNotFound),
            2 => Ok(EphemeralSplError::AlreadyInUse),
            3 => Ok(EphemeralSplError::EphemeralAtaMismatch),
            4 => Ok(EphemeralSplError::MintMismatch),
            5 => Ok(EphemeralSplError::VaultTokenAccountMismatch),
            6 => Ok(EphemeralSplError::TooManyAccountKeys),
            255 => Ok(EphemeralSplError::InfallibleError),
            _ => Err(ProgramError::InvalidArgument),
        }
    }
}
