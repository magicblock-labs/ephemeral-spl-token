#![no_std]
extern crate alloc;

mod entrypoint;
mod instruction;
mod processor;

pub use ephemeral_spl_api::ID;

pub use crate::entrypoint::process_instruction;

#[cfg(feature = "solana-security-txt")]
solana_security_txt::security_txt! {
    name: "MagicBlock Ephemeral SPL Token Program",
    project_url: "https://magicblock.xyz",
    contacts: "email:dev@magicblock.gg,twitter:@magicblock",
    policy: "https://github.com/magicblock-labs/ephemeral-spl-token/blob/main/LICENSE",
    preferred_languages: "en",
    source_code: "https://github.com/magicblock-labs/ephemeral-spl-token"
}
