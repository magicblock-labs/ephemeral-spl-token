#![no_std]
extern crate alloc;

mod entrypoint;
mod instruction;
mod processor;

pub use crate::entrypoint::process_instruction;
pub use ephemeral_spl_api::ID;

pub use ephemeral_spl_api::instructions::{
    AmountAndSaltArgs, CreateEphemeralAtaPermissionArgs, DelegateArgs, DelegateShuttleArgs,
    DepositAndDelegateShuttleArgs, DepositAndDelegateShuttleWithPrivateTransferAndStashCloseArgs,
    DepositAndDelegateShuttleWithPrivateTransferArgs, DepositAndQueueTransferArgs, DepositArgs,
    EnsureStealthPoolDelegatedArgs, ExecuteQueuedTransferArgs, ExecuteScheduledPrivateTransferArgs,
    InitializeShuttleEphemeralAtaArgs, InitializeTransferQueueArgs,
    ResetEphemeralAtaPermissionArgs, SchedulePrivateTransferArgs, UpdateStealthPoolArgs,
    WithdrawArgs,
};
