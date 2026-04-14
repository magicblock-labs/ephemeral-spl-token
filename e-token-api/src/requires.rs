///
/// require true
///
#[macro_export]
macro_rules! require {
    ($cond:expr, $error:expr) => {{
        if !$cond {
            let expr = stringify!($cond);
            pinocchio_log::log!("require!({}) failed.", expr);
            return Err($error.into());
        }
    }};
}

///
/// require_owned_by: a is owned by b
///
#[macro_export]
macro_rules! require_owned_by {
    ($account:expr, $owner_ref:expr) => {{
        if !pinocchio::address::address_eq(unsafe { $account.owner() }, $owner_ref) {
            pinocchio_log::log!(
                "require_owned_by!({}, {}) failed.",
                stringify!($account),
                stringify!($owner_ref)
            );
            $account.address().log();
            unsafe { $account.owner() }.log();
            $owner_ref.log();
            return Err(pinocchio::error::ProgramError::InvalidAccountOwner);
        }
    }};
}

///
/// require key1 == key2
///
#[macro_export]
macro_rules! require_eq_keys {
    ( $key1:expr, $key2:expr, $error:expr) => {{
        if !pinocchio::address::address_eq($key1, $key2) {
            pinocchio_log::log!(
                "require_eq_keys!({}, {}) failed: ",
                stringify!($key1),
                stringify!($key2)
            );
            $key1.log();
            $key2.log();
            return Err($error.into());
        }
    }};
}

///
/// require key1 != key2
///
#[macro_export]
macro_rules! require_ne_keys {
    ( $key1:expr, $key2:expr, $error:expr) => {{
        if pinocchio::address::address_eq($key1, $key2) {
            pinocchio_log::log!(
                "require_ne_keys!({}, {}) failed: ",
                stringify!($key1),
                stringify!($key2)
            );
            $key1.log();
            $key2.log();
            return Err($error.into());
        }
    }};
}

///
/// require a <= b
///
#[macro_export]
macro_rules! require_le {
    ( $val1:expr, $val2:expr, $error:expr) => {{
        if !($val1 <= $val2) {
            pinocchio_log::log!(
                "require_le!({}, {}) failed: {} <= {}",
                stringify!($val1),
                stringify!($val2),
                $val1,
                $val2
            );
            return Err($error.into());
        }
    }};
}

///
/// require a < b
///
#[macro_export]
macro_rules! require_lt {
    ( $val1:expr, $val2:expr, $error:expr) => {{
        if !($val1 < $val2) {
            pinocchio_log::log!(
                "require_lt!({}, {}) failed: {} < {}",
                stringify!($val1),
                stringify!($val2),
                $val1,
                $val2
            );
            return Err($error.into());
        }
    }};
}

///
/// require a == b
///
#[macro_export]
macro_rules! require_eq {
    ( $val1:expr, $val2:expr, $error:expr) => {{
        if !($val1 == $val2) {
            pinocchio_log::log!(
                "require_eq!({}, {}) failed: {} == {}",
                stringify!($val1),
                stringify!($val2),
                $val1,
                $val2
            );
            return Err($error.into());
        }
    }};
}

///
/// require a >= b
///
#[macro_export]
macro_rules! require_ge {
    ( $val1:expr, $val2:expr, $error:expr) => {{
        if !($val1 >= $val2) {
            pinocchio_log::log!(
                "require_ge!({}, {}) failed: {} >= {}",
                stringify!($val1),
                stringify!($val2),
                $val1,
                $val2
            );
            return Err($error.into());
        }
    }};
}

///
/// require a > b
///
#[macro_export]
macro_rules! require_gt {
    ( $val1:expr, $val2:expr, $error:expr) => {{
        if !($val1 > $val2) {
            pinocchio_log::log!(
                "require_gt!({}, {}) failed: {} > {}",
                stringify!($val1),
                stringify!($val2),
                $val1,
                $val2
            );
            return Err($error.into());
        }
    }};
}

///
/// require exactly n accounts
///
#[macro_export]
macro_rules! require_n_accounts {
    ( $accounts:expr, $n:literal) => {{
        match $accounts.len().cmp(&$n) {
            core::cmp::Ordering::Less => {
                pinocchio_log::log!(
                    "Need {} accounts, but got less ({}) accounts",
                    $n,
                    $accounts.len()
                );
                return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
            }
            core::cmp::Ordering::Equal => TryInto::<&[_; $n]>::try_into($accounts)
                .map_err(|_| $crate::error::EphemeralSplError::InfallibleError)?,
            core::cmp::Ordering::Greater => {
                pinocchio_log::log!(
                    "Need {} accounts, but got more ({}) accounts",
                    $n,
                    $accounts.len()
                );
                return Err($crate::error::EphemeralSplError::TooManyAccountKeys.into());
            }
        }
    }};
}

///
/// require n-or-more accounts, more is returned as slice.
///
#[macro_export]
macro_rules! require_n_accounts_with_optionals {
    ( $accounts:expr, $n:literal) => {{
        match $accounts.len().cmp(&$n) {
            core::cmp::Ordering::Less => {
                pinocchio_log::log!(
                    "Need {} accounts, but got less ({}) accounts",
                    $n,
                    $accounts.len()
                );
                return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
            }
            _ => {
                let (exact, optionals) = $accounts.split_at($n);

                (
                    TryInto::<&[_; $n]>::try_into(exact)
                        .map_err(|_| $crate::error::EphemeralSplError::InfallibleError)?,
                    optionals,
                )
            }
        }
    }};
}

///
/// require n-or-more accounts, more is ignored.
///
#[macro_export]
macro_rules! require_n_accounts_with_ignored {
    ( $accounts:expr, $n:literal) => {{
        match $accounts.len().cmp(&$n) {
            core::cmp::Ordering::Less => {
                pinocchio_log::log!(
                    "Need {} accounts, but got less ({}) accounts",
                    $n,
                    $accounts.len()
                );
                return Err(pinocchio::error::ProgramError::NotEnoughAccountKeys);
            }
            _ => {
                let (exact, _) = $accounts.split_at($n);
                TryInto::<&[_; $n]>::try_into(exact)
                    .map_err(|_| $crate::error::EphemeralSplError::InfallibleError)?
            }
        }
    }};
}

///
/// require Option to be Some
///
#[macro_export]
macro_rules! require_some {
    ($option:expr, $error:expr) => {{
        match $option {
            Some(val) => val,
            None => return Err($error.into()),
        }
    }};
}
