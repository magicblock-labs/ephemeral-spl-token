mod asserts;
mod ephemeral_account;
mod group_receipt_controller;
mod pda;
mod token;

pub use ephemeral_account::*;
use ephemeral_spl_api::debug_log;
pub use group_receipt_controller::*;
pub use pda::*;
pub use token::*;

pub fn print_id(id: &[u8; 3]) {
    let val: u32 = u32::from_le_bytes([id[0], id[1], id[2], 0]);
    debug_log!("yo yo id: {}", val);
}
