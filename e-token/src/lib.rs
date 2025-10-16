//! Another ERC20-like Token program for the Solana blockchain.

#![no_std]
extern crate alloc;

mod entrypoint;
mod processor;

pub use crate::entrypoint::process_instruction;
pub use ephemeral_spl_api::program::ID;
