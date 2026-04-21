use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::MaybeUninit;

use ephemeral_rollups_pinocchio::consts::{
    BUFFER, DELEGATION_METADATA, DELEGATION_PROGRAM_ID, DELEGATION_RECORD,
};
use ephemeral_spl_api::state::ephemeral_ata::EphemeralAta;
use ephemeral_spl_api::state::global_vault::GlobalVault;
use ephemeral_spl_api::state::shuttle_ephemeral_ata::ShuttleMetadata;
use ephemeral_spl_api::state::stash::StashPda;
use ephemeral_spl_api::state::transfer_queue::{TransferQueue, QUEUE_SEED};
use ephemeral_spl_api::{require, require_eq_keys, require_n_accounts};

use hydra_api::consts::{
    ix as hydra_ix, CRANK_HEADER_SIZE, CRANK_SEED_PREFIX, CRANKER_REWARD, META_FLAG_WRITABLE,
    SERIALIZED_META_SIZE,
};
use hydra_api::instruction::CREATE_FIXED_PREFIX_LEN;

use pinocchio::cpi::{invoke_signed_with_bounds, Seed, Signer};
use pinocchio::instruction::{InstructionAccount, InstructionView};
use pinocchio::sysvars::{rent::Rent, Sysvar};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use pinocchio_system::instructions::Transfer;

use crate::processor::initialize_rent_pda::{RENT_PDA_BUMP, RENT_PDA_SEED};
use crate::processor::process_scheduled_private_transfer::SCHEDULED_PT_ACCOUNTS;

/// Discriminator of the scheduled wrapper ix.
const SCHEDULED_IX_DISCRIMINATOR: u8 =
    ephemeral_spl_api::instruction::PROCESS_SCHEDULED_PRIVATE_TRANSFER;

/// `hydra::Create` CPI accounts: `[payer, crank, system_program]`.
const HYDRA_CREATE_CPI_ACCOUNTS: usize = 3;

/// Offsets into the fixed prefix of the instruction data.
const OFF_SHUTTLE_ID: usize = 0;
const OFF_STASH_BUMP: usize = 4;
const OFF_MINT: usize = 5;
const OFF_SHUTTLE_BUMP: usize = 37;
const OFF_SHUTTLE_EATA_BUMP: usize = 38;
const OFF_SHUTTLE_WALLET_ATA_BUMP: usize = 39;
const OFF_BUFFER_BUMP: usize = 40;
const OFF_DELEGATION_RECORD_BUMP: usize = 41;
const OFF_DELEGATION_METADATA_BUMP: usize = 42;
const OFF_GLOBAL_VAULT_BUMP: usize = 43;
const OFF_VAULT_TOKEN_BUMP: usize = 44;
const OFF_STASH_ATA_BUMP: usize = 45;
const OFF_QUEUE_BUMP: usize = 46;
const FIXED_PREFIX_LEN: usize = 47;

///
/// Executes on: BASE only. User-signed.
///
/// Appended to a swap transaction to schedule a private transfer
/// (instruction 25 via Hydra) over whatever balance ends up in the stash
/// ATA when the crank fires. Keeps the outer ix small: every account that
/// would only be read for its pubkey is derived on-chain using the bumps
/// supplied in the instruction data; hard-coded program IDs stand in for
/// DLP, system, ATA, and Hydra programs.
///
/// Accounts (7):
///
///  0: [signer]            - Keypair : User who owns the stash PDA.
///  1: [writable]          - PDA     : Stash PDA. Seeds `[b"stash", user, mint]`. Created if empty.
///  2: [writable]          - PDA     : Rent PDA. Funds crank + stash PDA rent.
///  3: [writable]          - PDA     : Hydra crank PDA. Derived from the stash PDA bytes.
///  4: []                  - Program : Hydra program.
///  5: []                  - Builtin : System program.
///  6: []                  - SPL     : Token program (Token / Token-2022) used as an ATA seed.
///
/// Instruction data (minimum 47 B fixed prefix + 3 vardata):
///
///   00..04  shuttle_id (u32 LE)
///   04      stash_pda_bump
///   05..37  mint (32 B)
///   37      shuttle_bump
///   38      shuttle_eata_bump
///   39      shuttle_wallet_ata_bump
///   40      buffer_bump
///   41      delegation_record_bump
///   42      delegation_metadata_bump
///   43      global_vault_bump
///   44      vault_token_bump
///   45      stash_ata_bump
///   46      queue_bump
///   47..    [len:u8] validator pubkey (0 or 32 bytes)
///   ....    [len:u8] encrypted destination (pass-through)
///   ....    [len:u8] encrypted data suffix (pass-through)
///
#[inline(never)]
pub fn process_schedule_private_transfer(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [
        user_info,
        stash_pda_info,
        rent_pda_info,
        hydra_crank_pda_info,
        _hydra_program_info, // present so Hydra program is in the tx account list for the CPI
        system_program_info,
        token_program_info,
    ] = require_n_accounts!(accounts, 7);

    require!(
        instruction_data.len() >= FIXED_PREFIX_LEN + 3,
        ProgramError::InvalidInstructionData
    );

    // -------- parse data --------
    let shuttle_id_bytes: [u8; 4] = instruction_data[OFF_SHUTTLE_ID..OFF_STASH_BUMP]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let stash_bump = instruction_data[OFF_STASH_BUMP];

    let mint = {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&instruction_data[OFF_MINT..OFF_SHUTTLE_BUMP]);
        Address::new_from_array(buf)
    };
    let shuttle_bump = instruction_data[OFF_SHUTTLE_BUMP];
    let shuttle_eata_bump = instruction_data[OFF_SHUTTLE_EATA_BUMP];
    let shuttle_wallet_ata_bump = instruction_data[OFF_SHUTTLE_WALLET_ATA_BUMP];
    let buffer_bump = instruction_data[OFF_BUFFER_BUMP];
    let delegation_record_bump = instruction_data[OFF_DELEGATION_RECORD_BUMP];
    let delegation_metadata_bump = instruction_data[OFF_DELEGATION_METADATA_BUMP];
    let global_vault_bump = instruction_data[OFF_GLOBAL_VAULT_BUMP];
    let vault_token_bump = instruction_data[OFF_VAULT_TOKEN_BUMP];
    let stash_ata_bump = instruction_data[OFF_STASH_ATA_BUMP];
    let queue_bump = instruction_data[OFF_QUEUE_BUMP];

    // Parse the three vardata blobs: validator, enc_dest, enc_suffix.
    let vardata_tail = &instruction_data[FIXED_PREFIX_LEN..];
    let parsed = VardataIter::new(vardata_tail);
    let validator_bytes = parsed.read_next()?;
    let enc_dest_offset = parsed.position();
    let _enc_dest = parsed.read_next_at(enc_dest_offset)?;
    let enc_suffix_offset = parsed.position();
    let _enc_suffix = parsed.read_next_at(enc_suffix_offset)?;

    require!(
        validator_bytes.is_empty() || validator_bytes.len() == 32,
        ProgramError::InvalidInstructionData
    );
    let validator = if validator_bytes.is_empty() {
        Address::default()
    } else {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(validator_bytes);
        Address::new_from_array(buf)
    };

    // Program-ID / signer / RENT_PDA checks are omitted — they're caught
    // implicitly by the downstream CPIs:
    //   • user-not-signer      → system Transfer fails (MissingSignature)
    //   • wrong rent_pda_info  → invoke_signed with rent seeds fails
    //   • wrong hydra_program  → Hydra program is not in the tx → CPI fails
    //   • wrong system_program → system Transfer CPI fails to dispatch
    //
    // The stash-PDA derivation is kept as a fail-fast safeguard: it
    // prevents a user from funding a non-canonical stash address and
    // burning `rent_pda`'s crank-rent on a crank that will never
    // successfully trigger.
    let hydra_program_id = Address::new_from_array(hydra_api::ID.to_bytes());
    let token_program_id = *token_program_info.address();

    let derived_stash = StashPda::derive_pda(user_info.address(), &mint, stash_bump)?;
    require_eq_keys!(
        &derived_stash,
        stash_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    // -------- fund stash PDA --------
    //
    // Single Transfer from user funds the stash PDA with `SETUP_LAMPORTS`.
    // That's > rent-exempt minimum for 0 bytes, so on first use the
    // account is implicitly created (owner = system_program, 0 data,
    // rent-exempt). At trigger time ix 25 drains exactly `SETUP_LAMPORTS`
    // back into `rent_pda`, leaving the stash PDA at 0 lamports — it
    // effectively ceases to exist between schedules, so there's no
    // permanent stash-PDA rent leak and no `close_stash` ix ever needed.
    const SETUP_LAMPORTS: u64 = ephemeral_spl_api::consts::SPONSORED_SHUTTLE_DELEGATION_SETUP_LAMPORTS
        + ephemeral_spl_api::consts::SPONSORED_SHUTTLE_PRIVATE_TRANSFER_EXTRA_LAMPORTS;

    Transfer {
        from: user_info,
        to: stash_pda_info,
        lamports: SETUP_LAMPORTS,
    }
    .invoke()?;

    let rent = Rent::get()?;
    let rent_bump_seed = [RENT_PDA_BUMP];
    let rent_signer_seeds = [Seed::from(RENT_PDA_SEED), Seed::from(&rent_bump_seed)];
    let rent_signer = Signer::from(&rent_signer_seeds);

    // -------- derive the other 10 pubkeys --------
    let shuttle =
        ShuttleMetadata::derive_pda(stash_pda_info.address(), &mint, {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(&shuttle_id_bytes);
            u32::from_le_bytes(buf)
        }, shuttle_bump)?;
    let shuttle_eata = EphemeralAta::derive_pda(&shuttle, &mint, shuttle_eata_bump)?;
    let shuttle_wallet_ata =
        derive_ata(&shuttle, &token_program_id, &mint, shuttle_wallet_ata_bump)?;
    let buffer = {
        let bump = [buffer_bump];
        Address::create_program_address(
            &[BUFFER, shuttle_eata.as_ref(), &bump],
            &crate::ID,
        )?
    };
    let delegation_record = {
        let bump = [delegation_record_bump];
        Address::create_program_address(
            &[DELEGATION_RECORD, shuttle_eata.as_ref(), &bump],
            &DELEGATION_PROGRAM_ID,
        )?
    };
    let delegation_metadata = {
        let bump = [delegation_metadata_bump];
        Address::create_program_address(
            &[DELEGATION_METADATA, shuttle_eata.as_ref(), &bump],
            &DELEGATION_PROGRAM_ID,
        )?
    };
    let global_vault = GlobalVault::derive_pda(&mint, global_vault_bump)?;
    let vault_token = derive_ata(&global_vault, &token_program_id, &mint, vault_token_bump)?;
    let stash_ata = derive_ata(
        stash_pda_info.address(),
        &token_program_id,
        &mint,
        stash_ata_bump,
    )?;
    let queue = TransferQueue::derive_pda(&mint, &validator, queue_bump)?;
    // Silence "field never read" on queue derivation in release: force usage.
    let _ = QUEUE_SEED;

    // -------- build Hydra scheduled_metas (19 entries, writable-flag-only) --------
    // Mirrors instruction 25's account layout. See
    // `process_scheduled_private_transfer.rs`.
    let sched_metas_writable: [bool; SCHEDULED_PT_ACCOUNTS] = [
        true,  //  0 stash_pda (payer slot)
        true,  //  1 rent_pda
        true,  //  2 shuttle
        true,  //  3 shuttle_eata
        true,  //  4 shuttle_wallet_ata
        false, //  5 stash_pda (owner slot; union writable with 0)
        false, //  6 owner_program
        true,  //  7 buffer
        true,  //  8 delegation_record
        true,  //  9 delegation_metadata
        false, // 10 delegation_program
        false, // 11 associated_token_program
        false, // 12 system_program
        false, // 13 mint
        false, // 14 token_program
        false, // 15 global_vault
        true,  // 16 stash_ata
        true,  // 17 vault_token
        true,  // 18 queue
    ];
    let sched_metas_keys: [&Address; SCHEDULED_PT_ACCOUNTS] = [
        stash_pda_info.address(),                //  0
        rent_pda_info.address(),                 //  1
        &shuttle,                                //  2
        &shuttle_eata,                           //  3
        &shuttle_wallet_ata,                     //  4
        stash_pda_info.address(),                //  5 duplicate of 0
        &crate::ID,                              //  6 owner_program = self
        &buffer,                                 //  7
        &delegation_record,                      //  8
        &delegation_metadata,                    //  9
        &DELEGATION_PROGRAM_ID,                  // 10
        &pinocchio_associated_token_account::ID, // 11
        system_program_info.address(),           // 12
        &mint,                                   // 13
        &token_program_id,                       // 14
        &global_vault,                           // 15
        &stash_ata,                              // 16
        &vault_token,                            // 17
        &queue,                                  // 18
    ];

    // -------- build Hydra scheduled_data --------
    // `[disc 29][user 32][stash_bump 1][shuttle_id 4][validator vardata][enc_dest vardata][enc_suffix vardata]`
    let scheduled_data_len = 1 + 32 + 1 + 4 + vardata_tail.len();
    let mut scheduled_data: Vec<u8> = Vec::with_capacity(scheduled_data_len);
    scheduled_data.push(SCHEDULED_IX_DISCRIMINATOR);
    scheduled_data.extend_from_slice(user_info.address().as_ref());
    scheduled_data.push(stash_bump);
    scheduled_data.extend_from_slice(&shuttle_id_bytes);
    scheduled_data.extend_from_slice(vardata_tail);

    // -------- derive Hydra crank PDA from stash PDA bytes --------
    let mut hydra_seed = [0u8; 32];
    hydra_seed.copy_from_slice(stash_pda_info.address().as_ref());

    let (derived_crank_pda, _crank_bump) =
        Address::find_program_address(&[CRANK_SEED_PREFIX, &hydra_seed], &hydra_program_id);
    require_eq_keys!(
        &derived_crank_pda,
        hydra_crank_pda_info.address(),
        ProgramError::InvalidSeeds
    );

    // -------- fund the crank (rent + one trigger reward) --------
    let region_len = 2 + SERIALIZED_META_SIZE * SCHEDULED_PT_ACCOUNTS + 32 + 2 + scheduled_data_len;
    let crank_account_size = CRANK_HEADER_SIZE + region_len;
    let crank_rent = rent.try_minimum_balance(crank_account_size)?;
    let crank_funding = crank_rent
        .checked_add(CRANKER_REWARD)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    Transfer {
        from: rent_pda_info,
        to: hydra_crank_pda_info,
        lamports: crank_funding,
    }
    .invoke_signed(&[rent_signer.clone()])?;

    // -------- build hydra::Create wire data --------
    let scheduled_metas_bytes = SERIALIZED_META_SIZE * SCHEDULED_PT_ACCOUNTS;
    let create_data_len = 1 + CREATE_FIXED_PREFIX_LEN + scheduled_metas_bytes + scheduled_data_len;
    let mut create_data: Vec<u8> = Vec::with_capacity(create_data_len);
    create_data.push(hydra_ix::CREATE);
    create_data.extend_from_slice(&hydra_seed);            // seed (32)
    create_data.extend_from_slice(rent_pda_info.address().as_ref()); // authority = rent_pda (lets us hydra::Cancel with rent-signer and refund)
    create_data.extend_from_slice(&0u64.to_le_bytes());    // start_slot (ASAP)
    create_data.extend_from_slice(&1u64.to_le_bytes());    // interval_slots
    create_data.extend_from_slice(&1u64.to_le_bytes());    // remaining = 1 (one-shot)
    create_data.extend_from_slice(&0u64.to_le_bytes());    // priority_tip
    create_data.extend_from_slice(&0u32.to_le_bytes());    // cu_limit (0 = default)
    create_data.push(SCHEDULED_PT_ACCOUNTS as u8);         // num_accounts
    create_data.extend_from_slice(&(scheduled_data_len as u16).to_le_bytes()); // data_len
    create_data.extend_from_slice(crate::ID.as_ref());     // scheduled program_id
    for i in 0..SCHEDULED_PT_ACCOUNTS {
        let flag = if sched_metas_writable[i] {
            META_FLAG_WRITABLE
        } else {
            0
        };
        create_data.push(flag);
        create_data.extend_from_slice(sched_metas_keys[i].as_ref());
    }
    create_data.extend_from_slice(&scheduled_data);

    // -------- CPI into hydra::Create --------
    let mut hydra_create_metas =
        [const { MaybeUninit::<InstructionAccount>::uninit() }; HYDRA_CREATE_CPI_ACCOUNTS];
    unsafe {
        hydra_create_metas
            .get_unchecked_mut(0)
            .write(InstructionAccount::writable_signer(rent_pda_info.address()));
        hydra_create_metas
            .get_unchecked_mut(1)
            .write(InstructionAccount::writable(
                hydra_crank_pda_info.address(),
            ));
        hydra_create_metas
            .get_unchecked_mut(2)
            .write(InstructionAccount::readonly(system_program_info.address()));
    }
    let hydra_create_ix = InstructionView {
        program_id: &hydra_program_id,
        accounts: unsafe {
            core::slice::from_raw_parts(
                hydra_create_metas.as_ptr() as *const InstructionAccount,
                HYDRA_CREATE_CPI_ACCOUNTS,
            )
        },
        data: &create_data,
    };
    let hydra_account_refs: [&AccountView; HYDRA_CREATE_CPI_ACCOUNTS] = [
        rent_pda_info,
        hydra_crank_pda_info,
        system_program_info,
    ];

    invoke_signed_with_bounds::<HYDRA_CREATE_CPI_ACCOUNTS>(
        &hydra_create_ix,
        &hydra_account_refs,
        &[rent_signer],
    )
}

/// Derive the associated-token-account PDA with a client-supplied bump.
///
/// Seeds: `[wallet, token_program, mint, bump]` under the ATA program.
#[inline(always)]
fn derive_ata(
    wallet: &Address,
    token_program: &Address,
    mint: &Address,
    bump_seed: u8,
) -> Result<Address, ProgramError> {
    let bump = [bump_seed];
    let pda = Address::create_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref(), &bump],
        &pinocchio_associated_token_account::ID,
    )?;
    Ok(pda)
}

/// Walks three back-to-back `[len:u8][bytes]` blobs without allocating.
///
/// Used interior-mutably via `Cell` — keeps the borrow checker off our back
/// while threading the cursor across three reads.
struct VardataIter<'a> {
    raw: *const u8,
    len: usize,
    cursor: core::cell::Cell<usize>,
    _data: PhantomData<&'a [u8]>,
}

impl<'a> VardataIter<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            raw: bytes.as_ptr(),
            len: bytes.len(),
            cursor: core::cell::Cell::new(0),
            _data: PhantomData,
        }
    }

    fn position(&self) -> usize {
        self.cursor.get()
    }

    fn read_next(&self) -> Result<&'a [u8], ProgramError> {
        self.read_next_at(self.cursor.get())
    }

    fn read_next_at(&self, offset: usize) -> Result<&'a [u8], ProgramError> {
        require!(offset < self.len, ProgramError::InvalidInstructionData);
        let field_len = unsafe { *self.raw.add(offset) } as usize;
        let end = offset
            .checked_add(1)
            .and_then(|v| v.checked_add(field_len))
            .ok_or(ProgramError::InvalidInstructionData)?;
        require!(end <= self.len, ProgramError::InvalidInstructionData);
        let slice = unsafe { core::slice::from_raw_parts(self.raw.add(offset + 1), field_len) };
        self.cursor.set(end);
        Ok(slice)
    }
}
