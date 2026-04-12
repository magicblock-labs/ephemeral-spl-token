#![no_std]
extern crate alloc;

mod entrypoint;
mod instruction;
mod processor;

pub use crate::entrypoint::process_instruction;
pub use ephemeral_spl_api::ID;

pub use processor::{
    DepositAndDelegateShuttleWithPrivateTransferArgs, DepositAndQueueTransferArgs,
    ExecuteQueuedTransferArgs, InitializeTransferQueueArgs,
};
