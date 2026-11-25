//! Native mock for the ACL permission program.
//!
//! The `acl.so` fixture predates ephemeral permissions, and the real ephemeral
//! flow allocates the permission account through the magic program builtin,
//! which `magic_mock` stubs as a no-op anyway. This mock validates account
//! metas, captures invocations for assertions, and succeeds — mirroring the
//! `magic_mock` capture style.

#![allow(dead_code)]

use std::sync::{Mutex, OnceLock};

use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, program_error::ProgramError};
use solana_program_test::ProgramTest;
use solana_pubkey::Pubkey;

const CREATE_PERMISSION_DISCRIMINATOR: u64 = 0;
const CREATE_EPHEMERAL_PERMISSION_DISCRIMINATOR: u64 = 6;
const CLOSE_EPHEMERAL_PERMISSION_DISCRIMINATOR: u64 = 8;

const MEMBER_SIZE: usize = 33; // flags(1) + pubkey(32)

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedMember {
    pub flags: u8,
    pub pubkey: Pubkey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCreateEphemeralPermission {
    pub payer: Pubkey,
    pub permissioned_account: Pubkey,
    pub permission: Pubkey,
    pub is_private: bool,
    pub members: Vec<CapturedMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCloseEphemeralPermission {
    pub payer: Pubkey,
    pub permissioned_account: Pubkey,
    pub permission: Pubkey,
}

fn captured_ephemeral_permission_creates() -> &'static Mutex<Vec<CapturedCreateEphemeralPermission>> {
    static S: OnceLock<Mutex<Vec<CapturedCreateEphemeralPermission>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

fn captured_ephemeral_permission_closes() -> &'static Mutex<Vec<CapturedCloseEphemeralPermission>> {
    static S: OnceLock<Mutex<Vec<CapturedCloseEphemeralPermission>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn take_captured_ephemeral_permission_creates() -> Vec<CapturedCreateEphemeralPermission> {
    std::mem::take(&mut *captured_ephemeral_permission_creates().lock().unwrap())
}

pub fn take_captured_ephemeral_permission_closes() -> Vec<CapturedCloseEphemeralPermission> {
    std::mem::take(&mut *captured_ephemeral_permission_closes().lock().unwrap())
}

pub fn clear_all_captured() {
    take_captured_ephemeral_permission_creates();
    take_captured_ephemeral_permission_closes();
}

pub fn process(_program_id: &Pubkey, accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    let discriminator_bytes: [u8; 8] = instruction_data
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .ok_or(ProgramError::InvalidInstructionData)?;

    match u64::from_le_bytes(discriminator_bytes) {
        // Fixture setup (queue / stealth pool init) creates base permissions;
        // nothing on-chain reads them back, so success is enough here.
        CREATE_PERMISSION_DISCRIMINATOR => Ok(()),
        CREATE_EPHEMERAL_PERMISSION_DISCRIMINATOR => {
            let [payer, permissioned_account, permission, _vault, _magic_program, ..] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };
            if !payer.is_signer || !payer.is_writable {
                return Err(ProgramError::MissingRequiredSignature);
            }
            if !permissioned_account.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            if !permission.is_writable {
                return Err(ProgramError::InvalidAccountData);
            }

            let args = &instruction_data[8..];
            let (&is_private, members_data) = args.split_first().ok_or(ProgramError::InvalidInstructionData)?;
            if members_data.len() % MEMBER_SIZE != 0 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let members = members_data
                .chunks_exact(MEMBER_SIZE)
                .map(|chunk| CapturedMember {
                    flags: chunk[0],
                    pubkey: Pubkey::new_from_array(chunk[1..].try_into().unwrap()),
                })
                .collect();

            captured_ephemeral_permission_creates()
                .lock()
                .unwrap()
                .push(CapturedCreateEphemeralPermission {
                    payer: *payer.key,
                    permissioned_account: *permissioned_account.key,
                    permission: *permission.key,
                    is_private: is_private != 0,
                    members,
                });
            Ok(())
        }
        CLOSE_EPHEMERAL_PERMISSION_DISCRIMINATOR => {
            let [payer, _authority, permissioned_account, permission, _vault, _magic_program, ..] = accounts else {
                return Err(ProgramError::NotEnoughAccountKeys);
            };
            if !payer.is_signer || !payer.is_writable {
                return Err(ProgramError::MissingRequiredSignature);
            }
            if !permissioned_account.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            if !permission.is_writable {
                return Err(ProgramError::InvalidAccountData);
            }

            captured_ephemeral_permission_closes()
                .lock()
                .unwrap()
                .push(CapturedCloseEphemeralPermission {
                    payer: *payer.key,
                    permissioned_account: *permissioned_account.key,
                    permission: *permission.key,
                });
            Ok(())
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

pub fn add_mock(pt: &mut ProgramTest) {
    use solana_program_test::processor;
    pt.prefer_bpf(false);
    pt.add_program("acl_mock", crate::utils::permission_program_id(), processor!(process));
    pt.prefer_bpf(true);
}
