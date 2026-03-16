#[cfg(feature = "logging")]
use alloc::string::ToString;

use alloc::vec::Vec;
use core::{marker::PhantomData, mem::MaybeUninit};
use ephemeral_rollups_pinocchio::{
    consts::{
        BUFFER, DELEGATION_PROGRAM_ID, MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID,
        MAX_POST_DELEGATION_SIGNERS,
    },
    instruction::{fill_seeds, ClearText, PostDelegationActions},
    types::{DelegateAccountArgs, DelegateConfig},
    utils::{close_pda_acc, make_seed_buf, serialize_delegate_with_actions_data},
};
use ephemeral_spl_api::state::{
    ephemeral_ata::EphemeralAta, load_mut_unchecked, load_unchecked,
    shuttle_ephemeral_ata::ShuttleEphemeralAta, Initializable,
};
use pinocchio::{
    cpi::{invoke_signed_with_bounds, Seed, Signer},
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Assign, CreateAccount, Transfer};
use pinocchio_token_2022::state::Mint;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

use crate::processor::{
    deposit_spl_tokens::transfer_to_vault_for_mint,
    initialize_shuttle_ephemeral_ata::initialize_shuttle_ephemeral_ata_with_sponsor,
    rent_pda::derive_rent_pda,
};

pub(crate) struct DepositAndDelegateShuttleAccounts<'a> {
    pub(crate) payer_info: &'a AccountView,
    pub(crate) rent_pda_info: &'a AccountView,
    pub(crate) shuttle_info: &'a AccountView,
    pub(crate) shuttle_eata_info: &'a AccountView,
    pub(crate) shuttle_wallet_ata_info: &'a AccountView,
    pub(crate) owner_info: &'a AccountView,
    pub(crate) owner_program: &'a AccountView,
    pub(crate) buffer_acc: &'a AccountView,
    pub(crate) delegation_record: &'a AccountView,
    pub(crate) delegation_metadata: &'a AccountView,
    pub(crate) system_program: &'a AccountView,
    pub(crate) destination_token_info: &'a AccountView,
    pub(crate) mint_info: &'a AccountView,
    pub(crate) token_program_info: &'a AccountView,
    pub(crate) global_vault_info: &'a AccountView,
    pub(crate) owner_source_token_info: &'a AccountView,
    pub(crate) vault_token_info: &'a AccountView,
}

pub(crate) struct PreparedShuttleDelegation {
    pub(crate) mint: Address,
    pub(crate) shuttle_eata_bump: u8,
    pub(crate) rent_bump: u8,
    pub(crate) already_delegated: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct DepositAndDelegateShuttleCommonArgs {
    pub(crate) shuttle_id: u32,
    pub(crate) shuttle_bump: u8,
    pub(crate) amount: u64,
    pub(crate) validator: Option<[u8; 32]>,
}

#[inline(never)]
pub fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let args = DepositAndDelegateShuttleArgs::try_from_bytes(instruction_data)?;
    let accounts = parse_deposit_and_delegate_shuttle_accounts(accounts)?;

    process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
        &accounts,
        args.common_args(),
        default_post_delegation_actions(&accounts),
    )
}

pub struct DepositAndDelegateShuttleArgs<'a> {
    raw: *const u8,
    len: usize,
    _data: PhantomData<&'a [u8]>,
}

impl DepositAndDelegateShuttleArgs<'_> {
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Result<DepositAndDelegateShuttleArgs<'_>, ProgramError> {
        if bytes.len() != 13 && bytes.len() != 45 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(DepositAndDelegateShuttleArgs {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            _data: PhantomData,
        })
    }

    #[inline]
    pub fn shuttle_id(&self) -> u32 {
        let mut buf = [0u8; 4];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw, buf.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(buf)
    }

    #[inline]
    pub fn shuttle_bump(&self) -> u8 {
        unsafe { *self.raw.add(4) }
    }

    #[inline]
    pub fn amount(&self) -> u64 {
        let mut buf = [0u8; 8];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(5), buf.as_mut_ptr(), 8);
        }
        u64::from_le_bytes(buf)
    }

    #[inline]
    pub fn validator(&self) -> Option<[u8; 32]> {
        if self.len == 13 {
            return None;
        }

        let mut validator = [0u8; 32];
        unsafe {
            core::ptr::copy_nonoverlapping(self.raw.add(13), validator.as_mut_ptr(), 32);
        }
        Some(validator)
    }

    #[inline]
    pub(crate) fn common_args(&self) -> DepositAndDelegateShuttleCommonArgs {
        DepositAndDelegateShuttleCommonArgs {
            shuttle_id: self.shuttle_id(),
            shuttle_bump: self.shuttle_bump(),
            amount: self.amount(),
            validator: self.validator(),
        }
    }
}

pub(crate) fn parse_deposit_and_delegate_shuttle_accounts(
    accounts: &[AccountView],
) -> Result<DepositAndDelegateShuttleAccounts<'_>, ProgramError> {
    let [payer_info, rent_pda_info, shuttle_info, shuttle_eata_info, shuttle_wallet_ata_info, owner_info, owner_program, buffer_acc, delegation_record, delegation_metadata, _delegation_program, _associated_token_program, system_program, destination_token_info, mint_info, token_program_info, global_vault_info, owner_source_token_info, vault_token_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    Ok(DepositAndDelegateShuttleAccounts {
        payer_info,
        rent_pda_info,
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        owner_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
        destination_token_info,
        mint_info,
        token_program_info,
        global_vault_info,
        owner_source_token_info,
        vault_token_info,
    })
}

pub(crate) fn default_post_delegation_actions(
    accounts: &DepositAndDelegateShuttleAccounts<'_>,
) -> Vec<Instruction> {
    alloc::vec![
        merge_shuttle_into_destination_action(accounts),
        undelegate_and_close_shuttle_action(accounts),
    ]
}

pub(crate) fn process_deposit_and_delegate_shuttle_ephemeral_ata_with_post_actions(
    accounts: &DepositAndDelegateShuttleAccounts<'_>,
    args: DepositAndDelegateShuttleCommonArgs,
    post_actions: Vec<Instruction>,
) -> ProgramResult {
    let prepared = prepare_sponsored_shuttle_delegation(
        accounts.payer_info,
        accounts.rent_pda_info,
        accounts.shuttle_info,
        accounts.shuttle_eata_info,
        accounts.shuttle_wallet_ata_info,
        accounts.owner_info,
        accounts.mint_info,
        accounts.token_program_info,
        accounts.system_program,
        args.shuttle_id,
        args.shuttle_bump,
    )?;

    if prepared.already_delegated {
        return Ok(());
    }

    transfer_to_vault_for_mint(
        accounts.global_vault_info,
        accounts.mint_info,
        accounts.owner_source_token_info,
        accounts.vault_token_info,
        accounts.owner_info,
        accounts.token_program_info,
        &prepared.mint,
        args.amount,
    )?;

    let shuttle_eata = unsafe {
        load_mut_unchecked::<EphemeralAta>(accounts.shuttle_eata_info.borrow_unchecked_mut())?
    };
    shuttle_eata.amount = shuttle_eata
        .amount
        .checked_add(args.amount)
        .ok_or(ProgramError::InvalidArgument)?;

    delegate_sponsored_shuttle_with_post_actions(
        accounts.payer_info,
        accounts.rent_pda_info,
        accounts.shuttle_info,
        accounts.shuttle_eata_info,
        accounts.owner_info,
        accounts.owner_program,
        accounts.buffer_acc,
        accounts.delegation_record,
        accounts.delegation_metadata,
        accounts.system_program,
        args,
        &prepared.mint,
        prepared.shuttle_eata_bump,
        prepared.rent_bump,
        post_actions,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_sponsored_shuttle_delegation(
    payer_info: &AccountView,
    rent_pda_info: &AccountView,
    shuttle_info: &AccountView,
    shuttle_eata_info: &AccountView,
    shuttle_wallet_ata_info: &AccountView,
    owner_info: &AccountView,
    mint_info: &AccountView,
    token_program_info: &AccountView,
    system_program: &AccountView,
    shuttle_id: u32,
    shuttle_bump: u8,
) -> Result<PreparedShuttleDelegation, ProgramError> {
    if !payer_info.is_signer() || !owner_info.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (derived_rent_pda, rent_bump) = derive_rent_pda();
    if derived_rent_pda != *rent_pda_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }
    if !rent_pda_info.owned_by(&pinocchio_system::ID) || rent_pda_info.data_len() != 0 {
        return Err(ProgramError::InvalidAccountData);
    }

    Transfer {
        from: payer_info,
        to: rent_pda_info,
        lamports:
            ephemeral_spl_api::consts::SETUP_AND_DELEGATE_SHUTTLE_EPHEMERAL_ATA_WITH_MERGE_PRICE_LAMPORTS,
    }
    .invoke()?;

    let rent_bump_seed = [rent_bump];
    let rent_signer_seed = [
        Seed::from(crate::processor::rent_pda::RENT_PDA_SEED),
        Seed::from(&rent_bump_seed),
    ];
    let rent_signer = Signer::from(&rent_signer_seed);

    initialize_shuttle_ephemeral_ata_with_sponsor(
        rent_pda_info,
        Some(rent_signer),
        shuttle_info,
        shuttle_eata_info,
        shuttle_wallet_ata_info,
        rent_pda_info,
        owner_info,
        mint_info,
        token_program_info,
        system_program,
        shuttle_id,
        shuttle_bump,
    )?;

    let delegation_program = ephemeral_spl_api::program::DELEGATION_PROGRAM_ID;
    if shuttle_eata_info.owned_by(&delegation_program) {
        return Ok(PreparedShuttleDelegation {
            mint: *mint_info.address(),
            shuttle_eata_bump: 0,
            rent_bump,
            already_delegated: true,
        });
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
    if shuttle.owner != *owner_info.address() || shuttle.payer != *rent_pda_info.address() {
        return Err(ProgramError::IncorrectAuthority);
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
        let shuttle_eata =
            unsafe { load_unchecked::<EphemeralAta>(shuttle_eata_info.borrow_unchecked())? };
        if !shuttle_eata.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if shuttle_eata.owner != *shuttle_info.address() {
            return Err(ProgramError::InvalidAccountData);
        }
        shuttle_eata.mint
    };

    if mint != *mint_info.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let (derived_shuttle_eata, shuttle_eata_bump) =
        ephemeral_spl_api::Address::find_program_address(
            &[shuttle_info.address().as_ref(), mint.as_ref()],
            &ephemeral_spl_api::program::id_address(),
        );
    if derived_shuttle_eata != *shuttle_eata_info.address() {
        return Err(ProgramError::InvalidSeeds);
    }

    Ok(PreparedShuttleDelegation {
        mint,
        shuttle_eata_bump,
        rent_bump,
        already_delegated: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn delegate_sponsored_shuttle_with_post_actions(
    payer_info: &AccountView,
    rent_pda_info: &AccountView,
    shuttle_info: &AccountView,
    shuttle_eata_info: &AccountView,
    owner_info: &AccountView,
    owner_program: &AccountView,
    buffer_acc: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
    system_program: &AccountView,
    args: DepositAndDelegateShuttleCommonArgs,
    mint: &Address,
    shuttle_eata_bump: u8,
    rent_bump: u8,
    post_actions: Vec<Instruction>,
) -> ProgramResult {
    let rent_bump_seed = [rent_bump];
    let rent_signer_seed = [
        Seed::from(crate::processor::rent_pda::RENT_PDA_SEED),
        Seed::from(&rent_bump_seed),
    ];
    let rent_signer = Signer::from(&rent_signer_seed);

    let seeds: &[&[u8]] = &[shuttle_info.address().as_ref(), mint.as_ref()];
    let config = DelegateConfig {
        validator: args.validator.map(Address::new_from_array),
        ..DelegateConfig::default()
    };
    let actions = post_actions.cleartext();
    let mut action_signer_accounts = alloc::vec![owner_info];
    if owner_info.address() != payer_info.address() {
        action_signer_accounts.push(payer_info);
    }

    #[cfg(feature = "logging")]
    {
        let shuttle_eata = shuttle_eata_info.address().to_string();
        pinocchio_log::log!("Shuttle eata: {}", shuttle_eata.as_str());
    }

    delegate_account_with_actions_from_sponsor(
        rent_pda_info,
        rent_signer,
        shuttle_eata_info,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
        seeds,
        shuttle_eata_bump,
        config,
        actions,
        &action_signer_accounts,
    )
}

#[inline(always)]
pub(crate) fn read_mint_decimals(mint_info: &AccountView) -> Result<u8, ProgramError> {
    let mint_data = unsafe { mint_info.borrow_unchecked() };
    if mint_data.len() < Mint::BASE_LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    let mint = unsafe { Mint::from_bytes_unchecked(mint_data) };
    if !mint.is_initialized() {
        return Err(ProgramError::UninitializedAccount);
    }

    Ok(mint.decimals())
}

fn merge_shuttle_into_destination_action(
    accounts: &DepositAndDelegateShuttleAccounts<'_>,
) -> Instruction {
    Instruction {
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
        accounts: alloc::vec![
            AccountMeta::new_readonly(pubkey(accounts.owner_info.address()), true),
            AccountMeta::new(pubkey(accounts.destination_token_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.shuttle_info.address()), false),
            AccountMeta::new(pubkey(accounts.shuttle_wallet_ata_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.mint_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.token_program_info.address()), false),
        ],
        data: alloc::vec![ephemeral_spl_api::instruction::MERGE_SHUTTLE_INTO_EPHEMERAL_ATA],
    }
}

fn undelegate_and_close_shuttle_action(
    accounts: &DepositAndDelegateShuttleAccounts<'_>,
) -> Instruction {
    Instruction {
        program_id: Pubkey::from(ephemeral_spl_api::program::ID),
        accounts: alloc::vec![
            AccountMeta::new(pubkey(accounts.payer_info.address()), true),
            AccountMeta::new(pubkey(accounts.rent_pda_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.shuttle_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.shuttle_eata_info.address()), false),
            AccountMeta::new(pubkey(accounts.shuttle_wallet_ata_info.address()), false),
            AccountMeta::new_readonly(pubkey(accounts.token_program_info.address()), false),
            AccountMeta::new(Pubkey::from(MAGIC_CONTEXT_ID.to_bytes()), false),
            AccountMeta::new_readonly(Pubkey::from(MAGIC_PROGRAM_ID.to_bytes()), false),
        ],
        data: alloc::vec![ephemeral_spl_api::instruction::UNDELEGATE_SHUTTLE_EPHEMERAL_ATA],
    }
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn delegate_account_with_actions_from_sponsor(
    sponsor_info: &AccountView,
    sponsor_signer: Signer<'_, '_>,
    pda_acc: &AccountView,
    owner_program: &AccountView,
    buffer_acc: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
    system_program: &AccountView,
    seeds: &[&[u8]],
    bump: u8,
    config: DelegateConfig,
    actions: PostDelegationActions,
    action_signer_accounts: &[&AccountView],
) -> ProgramResult {
    let pda_key_bytes = pda_acc.address().as_array();
    let (_, buffer_pda_bump) = ephemeral_spl_api::Address::find_program_address(
        &[BUFFER, pda_key_bytes.as_ref()],
        owner_program.address(),
    );

    let buffer_bump_slice = [buffer_pda_bump];
    let buffer_seed_binding = [
        Seed::from(BUFFER),
        Seed::from(pda_key_bytes.as_ref()),
        Seed::from(&buffer_bump_slice),
    ];
    let buffer_signer = Signer::from(&buffer_seed_binding);

    let data_len = pda_acc.data_len();

    CreateAccount {
        from: sponsor_info,
        to: buffer_acc,
        lamports: 0,
        space: data_len as u64,
        owner: owner_program.address(),
    }
    .invoke_signed(&[sponsor_signer.clone(), buffer_signer])?;

    {
        let pda_ro = pda_acc.try_borrow()?;
        let mut buf_data = buffer_acc.try_borrow_mut()?;
        buf_data.copy_from_slice(&pda_ro);
    }
    {
        let mut pda_mut = pda_acc.try_borrow_mut()?;
        for b in pda_mut.iter_mut().take(data_len) {
            *b = 0;
        }
    }

    let mut seed_buf = make_seed_buf();
    let filled = fill_seeds(&mut seed_buf, seeds, &bump);
    let delegate_signer = Signer::from(filled);

    let current_owner = unsafe { pda_acc.owner() };
    if current_owner != &pinocchio_system::ID {
        unsafe { pda_acc.assign(&pinocchio_system::ID) };
    }
    let current_owner = unsafe { pda_acc.owner() };
    if current_owner != &DELEGATION_PROGRAM_ID {
        Assign {
            account: pda_acc,
            owner: &DELEGATION_PROGRAM_ID,
        }
        .invoke_signed(&[delegate_signer.clone()])?;
    }

    let delegate_args = DelegateAccountArgs {
        commit_frequency_ms: config.commit_frequency_ms,
        seeds,
        validator: config.validator,
    };

    cpi_delegate_with_actions_from_sponsor(
        sponsor_info,
        pda_acc,
        owner_program,
        buffer_acc,
        delegation_record,
        delegation_metadata,
        system_program,
        delegate_args,
        actions,
        action_signer_accounts,
        &[sponsor_signer.clone(), delegate_signer],
    )?;

    close_pda_acc(sponsor_info, buffer_acc)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn cpi_delegate_with_actions_from_sponsor(
    sponsor_info: &AccountView,
    pda_acc: &AccountView,
    owner_program: &AccountView,
    buffer_acc: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
    system_program: &AccountView,
    delegate_args: DelegateAccountArgs,
    actions: PostDelegationActions,
    action_signer_accounts: &[&AccountView],
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    if action_signer_accounts.len() != actions.signers.len() {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    const MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS: usize = 7 + MAX_POST_DELEGATION_SIGNERS;
    const UNINIT_ACCOUNT: MaybeUninit<InstructionAccount> =
        MaybeUninit::<InstructionAccount>::uninit();
    let mut account_metas = [UNINIT_ACCOUNT; MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS];
    let num_accounts = 7 + action_signer_accounts.len();
    if num_accounts > MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS {
        return Err(ProgramError::InvalidArgument);
    }

    unsafe {
        account_metas
            .get_unchecked_mut(0)
            .write(InstructionAccount::writable_signer(sponsor_info.address()));
        account_metas
            .get_unchecked_mut(1)
            .write(InstructionAccount::writable_signer(pda_acc.address()));
        account_metas
            .get_unchecked_mut(2)
            .write(InstructionAccount::readonly(owner_program.address()));
        account_metas
            .get_unchecked_mut(3)
            .write(InstructionAccount::writable(buffer_acc.address()));
        account_metas
            .get_unchecked_mut(4)
            .write(InstructionAccount::writable(delegation_record.address()));
        account_metas
            .get_unchecked_mut(5)
            .write(InstructionAccount::writable(delegation_metadata.address()));
        account_metas
            .get_unchecked_mut(6)
            .write(InstructionAccount::readonly(system_program.address()));
    }

    let mut i = 0;
    while i < action_signer_accounts.len() {
        unsafe {
            account_metas
                .get_unchecked_mut(7 + i)
                .write(InstructionAccount::readonly_signer(
                    action_signer_accounts[i].address(),
                ));
        }
        i += 1;
    }

    let data = serialize_delegate_with_actions_data(delegate_args, actions)?;
    let mut account_refs: [&AccountView; MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS] =
        [sponsor_info; MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS];
    account_refs[0] = sponsor_info;
    account_refs[1] = pda_acc;
    account_refs[2] = owner_program;
    account_refs[3] = buffer_acc;
    account_refs[4] = delegation_record;
    account_refs[5] = delegation_metadata;
    account_refs[6] = system_program;

    let mut j = 0;
    while j < action_signer_accounts.len() {
        account_refs[7 + j] = action_signer_accounts[j];
        j += 1;
    }

    let instruction = InstructionView {
        program_id: &DELEGATION_PROGRAM_ID,
        accounts: unsafe {
            core::slice::from_raw_parts(
                account_metas.as_ptr() as *const InstructionAccount,
                num_accounts,
            )
        },
        data: &data,
    };

    invoke_signed_with_bounds::<MAX_DELEGATE_WITH_ACTIONS_ACCOUNTS>(
        &instruction,
        &account_refs[..num_accounts],
        signers,
    )?;

    Ok(())
}

#[inline]
pub(crate) fn pubkey(address: &Address) -> Pubkey {
    Pubkey::from(address.to_bytes())
}
