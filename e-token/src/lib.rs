//! Another ERC20-like Token program for the Solana blockchain.

#![no_std]

mod entrypoint;
mod processor;
mod error;

pub use ephemeral_spl_api::ID;
pub use crate::entrypoint::process_instruction;
