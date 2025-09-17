use pinocchio::program_error::{ProgramError, ToStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EphemeralSplError {
    // invalid instruction data
    InvalidInstruction,
}

impl From<EphemeralSplError> for ProgramError {
    fn from(e: EphemeralSplError) -> Self {
        Self::Custom(e as u32)
    }
}

impl ToStr for EphemeralSplError {
    fn to_str<E>(&self) -> &'static str
    where
        E: 'static + ToStr + TryFrom<u32>,
    {
        match self {
            EphemeralSplError::InvalidInstruction => "Error: Invalid instruction",
        }
    }
}

impl core::convert::TryFrom<u32> for EphemeralSplError {
    type Error = ProgramError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EphemeralSplError::InvalidInstruction),
            _ => Err(ProgramError::InvalidArgument),
        }
    }
}
