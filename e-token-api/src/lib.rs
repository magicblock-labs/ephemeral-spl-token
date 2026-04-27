#![no_std]
extern crate alloc;

/// Feature-gated debug logging.
///
/// `debug_log!({ ... })` executes the whole block only when the `logging`
/// feature is enabled. Use this form when log-only setup would otherwise do
/// unnecessary work in non-logging builds.
///
/// Example:
/// ```ignore
/// debug_log!({
///     let shuttle = shuttle_info.address().to_string();
///     pinocchio_log::log!("shuttle={}", shuttle.as_str());
/// });
/// ```
///
/// `debug_log!(...)` mirrors `pinocchio_log::log!(...)`, but expands only when
/// the `logging` feature is enabled. Use this form for simple log statements
/// whose arguments are already cheap to compute.
#[macro_export]
macro_rules! debug_log {
    ({ $($body:tt)* }) => {{
        #[cfg(feature = "logging")]
        {
            $($body)*
        }
    }};
    ($($arg:tt)*) => {{
        #[cfg(feature = "logging")]
        {
            pinocchio_log::log!($($arg)*);
        }
    }};
}

// Single source of truth for the e-ephemeral-token program ID.
// Keep this in a separate rlib crate so tests and clients can link it while
// the on-chain program crate stays cdylib-only.
pub mod consts;
pub mod error;
pub mod instruction;
pub mod requires;
pub mod state;
pub mod program {
    pub use ephemeral_rollups_pinocchio::consts::DELEGATION_PROGRAM_ID;
}

solana_address::declare_id!("SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2");

/// Re-exported `Address` type from solana-address for convenience.
/// Used throughout the codebase for account address representations.
pub use solana_address::Address;
