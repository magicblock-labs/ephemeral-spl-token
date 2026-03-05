use alloc::vec::Vec;
use borsh::{BorshDeserialize, BorshSerialize};
use ephemeral_rollups_pinocchio::instruction::{MaybeEncryptedInstruction, MaybeEncryptedPubkey};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct DelegateShuttleWithActionArgs {
    pub bump: u8,
    pub validator: Option<[u8; 32]>,
    pub ix: MaybeEncryptedInstruction,
    pub accounts: Vec<MaybeEncryptedPubkey>,
}
