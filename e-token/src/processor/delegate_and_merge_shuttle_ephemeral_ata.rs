use alloc::vec;
use ephemeral_rollups_pinocchio::consts::{MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID};
use ephemeral_rollups_pinocchio::instruction::{
    AccountMeta as CompactAccountMeta, DelegateAccountWithActionsCpiBuilder,
    MaybeEncryptedAccountMeta, MaybeEncryptedInstruction, MaybeEncryptedIxData,
    PostDelegationActions,
};
use ephemeral_rollups_pinocchio::types::DelegateConfig;
use ephemeral_spl_api::instruction;
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_unchecked, shuttle_ephemeral_ata::ShuttleEphemeralAta,
    Initializable,
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_token_2022::state::TokenAccount;

pub fn process_delegate_and_merge_shuttle_ephemeral_ata(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DelegateShuttleArgs::try_from_bytes(instruction_data)?;

    let [
        owner_info,          // [signer]   Owner (the EATA owner)
        owner_ata_info,      // [writable] Owner ATA account (ATA for [owner, mint])
        shuttle_info,        // []         Shuttle metadata account (PDA [owner, mint, shuttle_id])
        shuttle_ata_info,    // [writable] Shuttle ATA account (ATA for [shuttle_metadata, mint])
        shuttle_eata_info,   // [writable] Shuttle EATA account (PDA [shuttle_metadata, mint]) - signer via seeds
        owner_program,       // []         Owner program (the program owning the delegated PDA)
        buffer_acc,          // [writable] Buffer account (used by the delegation program)
        delegation_record,   // [writable] Delegation record account
        delegation_metadata, // [writable] Delegation metadata account
        _delegation_program, // []         Delegation program
        token_program_info,  // []         Token program
        system_program,      // []         System program
    ] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if shuttle_eata_info.owned_by(&delegation_program) {
        return Ok(());
    }

    unsafe {
        if shuttle_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let shuttle =
        unsafe { load_unchecked::<ShuttleEphemeralAta>(shuttle_info.borrow_unchecked())? };
    if !shuttle.is_initialized() {
        return Err(ProgramError::InvalidAccountData);
    }

    unsafe {
        if shuttle_eata_info
            .owner()
            .ne(&ephemeral_spl_api::program::id_address())
        {
            return Err(ProgramError::IllegalOwner);
        }
    }

    let mint = {
        let ephemeral_ata =
            unsafe { load_unchecked::<EphemeralAta>(shuttle_eata_info.borrow_unchecked())? };
        if !ephemeral_ata.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if ephemeral_ata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        #[allow(clippy::clone_on_copy)]
        let mint = ephemeral_ata.mint.clone();
        mint
    };

    let owner_ata = TokenAccount::from_account_view(owner_ata_info)?;
    if !owner_ata.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }
    if owner_ata.owner() != owner_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }
    if owner_ata.mint() != &mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let shuttle_wallet_ata = TokenAccount::from_account_view(shuttle_ata_info)?;
    if !shuttle_wallet_ata.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }
    if shuttle_wallet_ata.owner() != shuttle_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }
    if shuttle_wallet_ata.mint() != &mint {
        return Err(ProgramError::InvalidAccountData);
    }

    let bump = [args.bump()];
    let derived_ephemeral_ata = ephemeral_spl_api::Address::create_program_address(
        &[
            shuttle_info.address().as_ref(),
            mint.as_ref(),
            bump.as_ref(),
        ],
        &ephemeral_spl_api::program::id_address(),
    )
    .map_err(|_| ProgramError::InvalidSeeds)?;
    if derived_ephemeral_ata != *shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];

    let config = DelegateConfig {
        validator: args.validator().map(Address::new_from_array),
        ..DelegateConfig::default()
    };

    let actions = PostDelegationActions {
        signers: vec![owner_info.address().to_bytes()],
        non_signers: vec![
            owner_info.address().to_bytes().into(),
            owner_ata_info.address().to_bytes().into(),
            shuttle_info.address().to_bytes().into(),
            shuttle_eata_info.address().to_bytes().into(),
            shuttle_ata_info.address().to_bytes().into(),
            mint.to_bytes().into(),
            token_program_info.address().to_bytes().into(),
            MAGIC_CONTEXT_ID.to_bytes().into(),
            MAGIC_PROGRAM_ID.to_bytes().into(),
            crate::ID.into(),
        ],
        instructions: vec![
            MaybeEncryptedInstruction {
                program_id: 9,
                accounts: vec![
                    CompactAccountMeta::new(0, true).into(),
                    CompactAccountMeta::new(1, false).into(),
                    CompactAccountMeta::new(2, false).into(),
                    CompactAccountMeta::new(4, false).into(),
                    CompactAccountMeta::new_readonly(5, false).into(),
                    CompactAccountMeta::new_readonly(6, false).into(),
                ],
                data: MaybeEncryptedIxData {
                    prefix: vec![instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA],
                    suffix: vec![].into(),
                },
            },
            MaybeEncryptedInstruction {
                program_id: 9,
                accounts: vec![
                    CompactAccountMeta::new(0, true).into(),
                    CompactAccountMeta::new_readonly(2, false).into(),
                    CompactAccountMeta::new(3, false).into(),
                    MaybeEncryptedAccountMeta::ClearText(CompactAccountMeta::new(4, false)),
                    CompactAccountMeta::new_readonly(6, false).into(),
                    CompactAccountMeta::new(7, false).into(),
                    CompactAccountMeta::new_readonly(8, false).into(),
                ],
                data: MaybeEncryptedIxData {
                    prefix: vec![instruction::UNDELEGATE_AND_CLOSE_SHUTTLE_EPHEMERAL_ATA],
                    suffix: vec![].into(),
                },
            },
        ],
    };

    #[cfg(feature = "logging")]
    {
        pinocchio_log::log!("Delegating shuttle");
    }

    DelegateAccountWithActionsCpiBuilder::new(
        owner_info,
        shuttle_eata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
    )
    .seeds(seeds)
    .bump(args.bump())
    .config(config)
    .actions(actions)
    .invoke()
}

pub struct DelegateShuttleArgs {
    bump: u8,
    validator: Option<[u8; 32]>,
}

impl DelegateShuttleArgs {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DelegateShuttleArgs, ProgramError> {
        if bytes.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let bump = bytes[0];
        let rest = &bytes[1..];
        let validator = if rest.is_empty() {
            None
        } else if rest.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(rest);
            Some(arr)
        } else {
            return Err(ProgramError::InvalidInstructionData);
        };
        Ok(DelegateShuttleArgs { bump, validator })
    }

    #[inline]
    pub fn validator(&self) -> Option<[u8; 32]> {
        self.validator
    }

    #[inline]
    pub fn bump(&self) -> u8 {
        self.bump
    }
}
