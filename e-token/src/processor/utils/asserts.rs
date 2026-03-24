#[macro_export]
macro_rules! assert_owner {
    ($account:expr, $expected_owner:expr) => {
        unsafe {
            if !pinocchio::address::address_eq($account.owner(), $expected_owner) {
                return Err(pinocchio::error::ProgramError::InvalidAccountOwner);
            }
        }
    };
}

#[macro_export]
macro_rules! assert_signer {
    ($account:expr) => {
        if !$account.is_signer() {
            return Err(pinocchio::error::ProgramError::MissingRequiredSignature);
        }
    };
}

#[macro_export]
macro_rules! assert_associated_token_address {
    ($ata:expr, $mint:expr, $wallet:expr, $token_program:expr) => {
        if $ata
            != &crate::processor::utils::get_associated_token_address(
                $wallet,
                $mint,
                $token_program,
            )
        {
            return Err(ProgramError::InvalidAccountData);
        }
    };
}
