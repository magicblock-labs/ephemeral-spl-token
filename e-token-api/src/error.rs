use pinocchio::error::{ProgramError, ToStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EphemeralSplError {
    // invalid instruction data
    InvalidInstruction,
    // account already initialized / in use
    AlreadyInUse,
    // Ephemeral ATA, Vault, Mint or Owner mismatch
    EphemeralAtaMismatch,
}

impl From<EphemeralSplError> for ProgramError {
    fn from(e: EphemeralSplError) -> Self {
        Self::Custom(e as u32)
    }
}

impl ToStr for EphemeralSplError {
    fn to_str(&self) -> &'static str {
        match self {
            EphemeralSplError::InvalidInstruction => "Error: Invalid instruction",
            EphemeralSplError::AlreadyInUse => "Error: Account already in use",
            EphemeralSplError::EphemeralAtaMismatch => {
                "Error: Ephemeral ATA/Vault/Mint/Owner mismatch"
            }
        }
    }
}

impl core::convert::TryFrom<u32> for EphemeralSplError {
    type Error = ProgramError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EphemeralSplError::InvalidInstruction),
            1 => Ok(EphemeralSplError::AlreadyInUse),
            2 => Ok(EphemeralSplError::EphemeralAtaMismatch),
            _ => Err(ProgramError::InvalidArgument),
        }
    }
}
