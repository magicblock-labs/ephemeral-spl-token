#[macro_export]
macro_rules! assert_owner {
    ($account:expr, $expected_owner:expr) => {
        let account = $account;
        let expected_owner = $expected_owner;
        let actual_owner = unsafe { account.owner() };
        if !pinocchio::address::address_eq(actual_owner, expected_owner) {
            return Err(pinocchio::error::ProgramError::InvalidAccountOwner);
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
        if !pinocchio::address::address_eq(
            $ata,
            &$crate::processor::utils::get_associated_token_address($wallet, $mint, $token_program),
        ) {
            return Err(pinocchio::error::ProgramError::InvalidAccountData);
        }
    };
}
