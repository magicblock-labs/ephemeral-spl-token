use borsh::{BorshDeserialize, BorshSerialize};
use ephemeral_rollups_pinocchio::instruction::PostDelegationActions;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct DelegateShuttleWithActionArgs {
    pub bump: u8,
    pub validator: Option<[u8; 32]>,
    pub encrypted_actions: PostDelegationActions,
}
