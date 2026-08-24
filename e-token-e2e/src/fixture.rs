//! On-chain setup shared by the e2e tests: program ids, wallet delegation, a
//! mint, and a transfer queue delegated to the rollup.
//!
//! Everything a real client would send comes from `ephemeral-rollups-sdk` —
//! its `spl::builders` for the program's instructions, its `spl` PDA helpers
//! for the addresses, and `dlp_api` for delegation and encryption. Only what
//! the published SDK does not carry is built here, and each of those says why.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ephemeral_rollups_sdk::{
    consts::{ASSOCIATED_TOKEN_PROGRAM_ID, DELEGATION_PROGRAM_ID, ESPL_TOKEN_PROGRAM_ID, TOKEN_PROGRAM_ID},
    dlp_api::{
        args::DelegateArgs, encryption::encrypt_ed25519_recipient, instruction_builder::delegate,
        pda::magic_fee_vault_pda_from_validator,
    },
    pda::{
        delegate_buffer_pda_from_delegated_account_and_owner_program, delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account,
    },
    spl::{
        builders::{
            DelegateEphemeralAtaBuilder, DelegateTransferQueueBuilder, DepositSplTokensBuilder,
            InitializeEphemeralAtaBuilder, InitializeGlobalVaultBuilder, InitializeRentPdaBuilder,
            InitializeTransferQueueBuilder,
        },
        find_rent_pda, find_shuttle_ata, find_shuttle_ephemeral_ata, find_shuttle_wallet_ata, find_transfer_queue,
        find_vault_ata, GlobalVault,
    },
};
use ephemeral_spl_api::{
    instruction::ESplInstruction,
    instructions::DepositAndDelegateShuttleWithPrivateTransferArgs,
    state::transfer_queue::{TransferQueueHeader, HEADER_LEN},
};
use solana_client::rpc_client::RpcClient;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_program_pack::Pack;
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use spl_token_interface::{
    instruction::{initialize_mint2, mint_to},
    state::Mint,
};
use wheels::layout::Encodable as _;

use crate::rpc::{account_data, airdrop, send, wait_for};

pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// The ephemeral SPL token program under test.
pub const PROGRAM_ID: Pubkey = ESPL_TOKEN_PROGRAM_ID;
/// ACL / permission program (`e-token/tests/fixtures/acl.so`).
pub use ephemeral_rollups_sdk::consts::PERMISSION_PROGRAM_ID;
/// Hydra's rollup-side program (`e-token/tests/fixtures/hydra_ephemeral.so`),
/// which the rollup's task scheduler creates cranks through. Not preloaded by
/// `mb-test-validator`, so the base has to carry it for the rollup to clone.
///
/// The SDK's `HYDRA_PROGRAM_ID` is the base-layer Hydra program; this is the
/// build that runs inside the rollup, and they are different addresses.
pub const HYDRA_EPHEMERAL_PROGRAM_ID: Pubkey = pubkey!("eHyd5BU8QffvHi4GnXwxrK4WpS7pM2x9UGKHBWii7mf");
pub const SYSTEM_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");

/// Surplus lamports the transfer queue holds so it can sponsor the ephemeral
/// group receipts `DepositAndQueueTransfer` creates.
pub const QUEUE_SPONSOR_LAMPORTS: u64 = LAMPORTS_PER_SOL;

/// The ephemeral-validator's default identity (from its bundled keypair). The
/// transfer queue must be delegated to this validator for the rollup to adopt
/// it. Logged at rollup startup as "Validator identity".
///
/// Not `dlp_api`'s `DEFAULT_VALIDATOR_IDENTITY`, which is a different key.
pub const ER_VALIDATOR_IDENTITY: Pubkey = pubkey!("mAGicPQYBMvcYveUZA5F5UNNwyHvfYh5xkLS2Fr1mev");

/// Make `account` an **on-curve delegated** account so the rollup will let its
/// own lamports change during a transaction. On the rollup, an account whose
/// lamports change must be delegated — for a fee payer specifically, a
/// non-delegated write is rejected with `InvalidAccountForFee`. The supported
/// path is the on-curve delegation flow: the wallet `assign`s itself to the
/// delegation program, then is delegated to the validator, in one base-layer
/// transaction. The rollup restores the original (system) owner when it clones
/// the account, so a delegated on-curve wallet is still a valid fee payer there.
///
/// Empty `seeds` are what make it on-curve — the delegation program then skips
/// the PDA seed check — and pinning the validator is what makes this rollup
/// adopt the delegation.
///
/// A separate system-owned `fee_payer` covers this base-layer transaction fee:
/// after `assign`, `account` is owned by the delegation program on the base and
/// can be neither a fee payer nor a `system_program::transfer` source.
pub fn delegate_wallet(base: &RpcClient, account: &Keypair, fee_payer: &Keypair) -> Result<()> {
    let acct = account.pubkey();

    // The wallet reassigns its own owner to the delegation program (it signs).
    let assign_ix = system_instruction::assign(&acct, &DELEGATION_PROGRAM_ID);
    let delegate_ix = delegate(
        fee_payer.pubkey(),
        acct,
        None,
        DelegateArgs {
            commit_frequency_ms: u32::MAX,
            seeds: vec![],
            validator: Some(ER_VALIDATOR_IDENTITY),
        },
    );

    send(
        base,
        &[assign_ix, delegate_ix],
        &fee_payer.pubkey(),
        &[fee_payer, account],
    )
    .with_context(|| format!("delegate {acct} (on-curve)"))
}

/// Create and initialize an SPL mint owned by `payer`.
pub fn create_mint(base: &RpcClient, payer: &Keypair, mint: &Keypair, decimals: u8) -> Result<()> {
    let rent = base
        .get_minimum_balance_for_rent_exemption(Mint::LEN)
        .context("mint rent")?;
    let create = system_instruction::create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        rent,
        Mint::LEN as u64,
        &TOKEN_PROGRAM_ID,
    );
    let init = initialize_mint2(&TOKEN_PROGRAM_ID, &mint.pubkey(), &payer.pubkey(), None, decimals)
        .context("build InitializeMint2")?;

    send(base, &[create, init], &payer.pubkey(), &[payer, mint]).context("create mint")
}

/// Create `owner`'s associated token account for `mint` (idempotent) and mint
/// `amount` into it.
pub fn create_ata_and_mint(
    base: &RpcClient,
    payer: &Keypair,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
) -> Result<Pubkey> {
    // `find_vault_ata` is named after the one caller the SDK has inside the
    // program; it is the ordinary ATA derivation for any wallet.
    let ata = find_vault_ata(mint, owner);
    // Built by hand rather than with `spl-associated-token-account-interface`:
    // that crate is still on the pre-`Address` pubkey type, so its builders
    // return an `Instruction` that is a different type from every other one here.
    let create = Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data: vec![1], // CreateIdempotent
    };

    let mut ixs = vec![create];
    if amount > 0 {
        ixs.push(mint_to(&TOKEN_PROGRAM_ID, mint, &ata, &payer.pubkey(), &[], amount).context("build MintTo")?);
    }

    send(base, &ixs, &payer.pubkey(), &[payer]).context("create ata + mint")?;
    Ok(ata)
}

/// Seed for the per-`(queue, source, group_id)` group receipt.
pub const GROUP_RECEIPT_SEED: &[u8] = b"group-receipt";

/// Derive the group receipt for `(queue, source, group_id)`.
///
/// Deliberately not the SDK's `find_transfer_group_receipt`: that seeds the PDA
/// with the low **three** bytes of the group id, while this program's
/// `derive_group_receipt_id` seeds it with the whole `u32`. The high byte is
/// always zero, but a three-byte seed and a four-byte one are still different
/// seeds, and they hash to different addresses.
pub fn derive_group_receipt(queue: &Pubkey, source: &Pubkey, group_id: u32) -> Pubkey {
    Pubkey::find_program_address(
        &[
            GROUP_RECEIPT_SEED,
            queue.as_ref(),
            source.as_ref(),
            &group_id.to_le_bytes(),
        ],
        &PROGRAM_ID,
    )
    .0
}

/// Build `InitializeEphemeralAta` + `DelegateEphemeralAta` for `user`.
///
/// An eATA is how a wallet gets a *writable* token account on the rollup. With
/// one delegated, the rollup projects it over the wallet's ATA and marks that
/// copy writable; without one, the rollup only ever sees the base ATA read-only.
/// The two balances are independent by design — the eATA is the rollup-side
/// balance, and nothing requires it to match what the wallet holds on the base.
///
/// The sender needs this because ix 25's `MergeShuttleIntoEphemeralAta`
/// post-action declares their token account **writable**. Without a delegated
/// eATA the whole bundle is rejected by MagicRoot's post-finalize guard with
/// `Immutable`, and the transfer is never queued.
///
/// `deposit` is credited to the eATA before it is delegated — that is the
/// rollup-side balance, and it has to be non-zero for anything the test spends
/// there. Delegation comes last because the deposit happens on the base.
///
/// `DelegateEphemeralAta`'s signer is also the eATA's first PDA seed, so `user`
/// signs it rather than `payer`. Pinning the validator makes the delegation
/// record's authority the identity that has to decrypt and run the post-actions.
pub fn setup_ephemeral_ata_ixs(
    payer: &Pubkey,
    user: &Pubkey,
    mint: &Pubkey,
    validator: &Pubkey,
    deposit: u64,
) -> Vec<Instruction> {
    vec![
        InitializeEphemeralAtaBuilder {
            payer: *payer,
            user: *user,
            mint: *mint,
        }
        .instruction(),
        DepositSplTokensBuilder {
            authority: *user,
            user: *user,
            mint: *mint,
            amount: deposit,
        }
        .instruction(),
        DelegateEphemeralAtaBuilder {
            payer: *user,
            user: *user,
            mint: *mint,
            validator: Some(*validator),
        }
        .instruction(),
    ]
}

/// Lamports the rent PDA is topped up to. It sponsors every ATA settlement
/// creates, so it must hold well more than one account's rent exemption.
pub const RENT_PDA_TARGET_LAMPORTS: u64 = 100_000_000;

/// Make sure the global rent PDA exists and is funded enough to sponsor the
/// account creations the flow depends on (notably the recipient ATA that
/// settlement creates).
///
/// The rent PDA is deliberately **System-owned with zero data** — that is what
/// `InitializeRentPda` checks for, so do not mistake it for uninitialized. Two
/// wrinkles make this worth doing carefully rather than blindly:
///
///  * it is a global singleton (`["rent"]`), so on a long-lived validator it
///    usually already exists and `InitializeRentPda` returns early;
///  * `InitializeRentPda` requires `lamports == 0` when it does create it, so an
///    account holding a partial balance can neither be initialized nor used.
///    Topping it up *first* turns that dead end into a working rent PDA.
pub fn ensure_rent_pda_funded(base: &RpcClient, payer: &Keypair) -> Result<u64> {
    let pda = find_rent_pda().0;
    let minimum = base
        .get_minimum_balance_for_rent_exemption(0)
        .context("rent-exempt minimum for an empty account")?;
    let before = base.get_balance(&pda).unwrap_or(0);

    // Fund first: it makes an already-existing account valid without touching
    // `InitializeRentPda`, and it rescues a partially-funded one.
    if before < RENT_PDA_TARGET_LAMPORTS {
        send(
            base,
            &[system_instruction::transfer(
                &payer.pubkey(),
                &pda,
                RENT_PDA_TARGET_LAMPORTS - before,
            )],
            &payer.pubkey(),
            &[payer],
        )
        .context("top up the rent PDA")?;
    }

    // Only create it if it does not exist yet; on an existing PDA this returns
    // early, and on a funded-but-uncreated one it would fail, which the top-up
    // above has already made unnecessary.
    let owner_of = |rpc: &RpcClient| {
        rpc.get_account_with_commitment(&pda, rpc.commitment())
            .ok()
            .and_then(|r| r.value)
            .map(|a| a.owner)
    };
    if owner_of(base).is_none() {
        send(
            base,
            &[InitializeRentPdaBuilder { payer: payer.pubkey() }.instruction()],
            &payer.pubkey(),
            &[payer],
        )
        .context("initialize the rent PDA")?;
    }

    let after = base.get_balance(&pda).unwrap_or(0);
    let owner = owner_of(base);
    let data_len = account_data(base, &pda).map(|d| d.len()).unwrap_or(0);

    // Assert the shape the program insists on, so a broken rent PDA is reported
    // here rather than as a mystifying failure deep in the shuttle flow.
    if owner != Some(SYSTEM_PROGRAM_ID) || data_len != 0 || after < minimum {
        bail!(
            "rent PDA {pda} is unusable: owner={owner:?} data_len={data_len} lamports={after};              the program requires System-owned, zero data, and at least {minimum} lamports"
        );
    }
    Ok(after)
}

/// An ix-25 instruction plus the group it will be queued under.
///
/// The group id is not a parameter — the program reads it out of the encrypted
/// destination — so the only way a caller can see it is to be handed it back
/// from here. The receipt itself needs no setup: `DepositAndQueueTransfer`
/// derives its address on the rollup and creates the account there.
pub struct PrivateTransfer {
    pub ix: Instruction,
    pub group_id: u32,
}

/// Build `DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer`
/// (ix 25) — the single base-layer instruction a real client sends to make a
/// private queued transfer.
///
/// It deposits `amount` into the global vault, then delegates a short-lived
/// *shuttle* to the rollup carrying post-actions. The middle post-action is the
/// `DepositAndQueueTransfer` that actually enqueues, and it can only run there:
/// the rollup grants write access to the shuttle's token account through the
/// delegation's action list, which is the only way an SPL token account is
/// writable on the rollup at all.
///
/// The destination and the queue parameters are encrypted to the validator with
/// [`encrypt_ed25519_recipient`] — libsodium's sealed box, the same code the
/// rollup decrypts with — so the base layer never sees who is being paid.
///
/// Built here rather than with the SDK's
/// `DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransferBuilder`:
/// that builder is a release behind this program's argument layout. It omits
/// `exact_out` and length-prefixes the destination, where the program takes
/// `exact_out` and a fixed `[u8; 80]` (see
/// [`DepositAndDelegateShuttleWithPrivateTransferArgs`]). The account list is
/// unchanged, so only the data differs — and it is encoded through the
/// program's own layout type rather than packed byte by byte.
#[allow(clippy::too_many_arguments)]
pub fn private_queued_transfer_ix(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    validator: &Pubkey,
    destination_owner: &Pubkey,
    shuttle_id: u32,
    amount: u64,
    min_delay_ms: u64,
    max_delay_ms: u64,
) -> Result<PrivateTransfer> {
    let shuttle = find_shuttle_ephemeral_ata(owner, mint, shuttle_id).0;
    let shuttle_eata = find_shuttle_ata(&shuttle, mint).0;
    let shuttle_wallet_ata = find_shuttle_wallet_ata(mint, &shuttle);
    // `transfer_queue_tick` derives the vault from `[mint]`, so the deposit has
    // to land in *this* vault's ATA for the crank to be able to settle from it.
    let vault = GlobalVault::find_pda(mint).0;
    let queue = find_transfer_queue(mint, validator).0;

    let buffer = delegate_buffer_pda_from_delegated_account_and_owner_program(&shuttle_eata, &PROGRAM_ID);
    let record = delegation_record_pda_from_delegated_account(&shuttle_eata);
    let metadata = delegation_metadata_pda_from_delegated_account(&shuttle_eata);

    // The program's destination argument is a fixed `[u8; 80]`: a 32-byte pubkey
    // plus the sealed box's 32-byte ephemeral public key and 16-byte Poly1305 tag.
    let encrypted_destination: [u8; 80] =
        encrypt_ed25519_recipient(&destination_owner.to_bytes(), &validator.to_bytes())?
            .try_into()
            .map_err(|got: Vec<u8>| anyhow!("encrypted destination must be 80 bytes, got {}", got.len()))?;
    // The program takes the group id from the first three bytes of the encrypted
    // destination, so it is only knowable from the very ciphertext this
    // instruction will carry.
    let group_id = u32::from(encrypted_destination[0])
        | (u32::from(encrypted_destination[1]) << 8)
        | (u32::from(encrypted_destination[2]) << 16);

    // The queue parameters ride along in a second sealed box. Layout matches
    // `pack_private_transfer_suffix` in the SDK; a single split and no client
    // ref id, so the plaintext is a flat 20 bytes.
    let mut suffix = Vec::with_capacity(20);
    suffix.extend_from_slice(&min_delay_ms.to_le_bytes());
    suffix.extend_from_slice(&max_delay_ms.to_le_bytes());
    suffix.extend_from_slice(&1u32.to_le_bytes()); // split
    let encrypted_data_suffix = encrypt_ed25519_recipient(&suffix, &validator.to_bytes())?;

    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(find_rent_pda().0, false),
            AccountMeta::new(shuttle, false),
            AccountMeta::new(shuttle_eata, false),
            AccountMeta::new(shuttle_wallet_ata, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(PROGRAM_ID, false),
            AccountMeta::new(buffer, false),
            AccountMeta::new(record, false),
            AccountMeta::new(metadata, false),
            AccountMeta::new_readonly(DELEGATION_PROGRAM_ID, false),
            AccountMeta::new_readonly(ASSOCIATED_TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new(find_vault_ata(mint, owner), false),
            AccountMeta::new(find_vault_ata(mint, &vault), false),
            AccountMeta::new(queue, false),
        ],
        data: ESplInstruction::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer.with_data(
            &DepositAndDelegateShuttleWithPrivateTransferArgs {
                shuttle_id,
                amount,
                exact_out: true,
                encrypted_destination,
                validator: Some(*validator),
                encrypted_data_suffix,
            }
            .encode()?,
        ),
    };
    Ok(PrivateTransfer { ix, group_id })
}

/// A mint plus its transfer queue, set up on the base and delegated to the
/// rollup. `validator` is always the rollup's identity.
pub struct QueueFixture {
    /// Delegated on the rollup, so it can pay fees and be written there. Also
    /// acts as the depositing sender.
    pub payer: Keypair,
    /// System-owned on the base, for any further base-layer transactions.
    pub helper: Keypair,
    /// Delegated on the rollup; pays trigger fees and collects crank rewards.
    pub cranker: Keypair,
    pub mint: Pubkey,
    pub validator: Pubkey,
    pub queue: Pubkey,
    /// Per-mint vault that deposits land in and settlement pays out of.
    pub vault: Pubkey,
    pub vault_ata: Pubkey,
    /// The sender's token account on the **base**, holding
    /// [`Self::initial_balance`]. Their rollup balance is separate and lives in
    /// their delegated eATA — see [`setup_ephemeral_ata_ixs`].
    pub sender_ata: Pubkey,
    /// Whoever the queued transfer is destined for. Its ATA is created by
    /// settlement, so it does not exist up front.
    pub recipient: Pubkey,
    pub recipient_ata: Pubkey,
    pub initial_balance: u64,
}

/// Fund the wallets, create a mint, initialize its transfer queue, and delegate
/// the queue to the rollup. Returns once the rollup can see the queue.
///
/// `payer` comes back **delegated**: it pays fees on the rollup, and the rollup
/// only lets a delegated account's lamports change. Delegation is therefore the
/// last step — once a wallet is assigned to the delegation program it can no
/// longer be a fee payer on the base, so all base-layer setup has to happen
/// first, paid for by a throwaway `helper` wallet that stays system-owned.
pub fn setup_queue(base: &RpcClient, er: &RpcClient, decimals: u8) -> Result<QueueFixture> {
    let payer = Keypair::new();
    let helper = Keypair::new();
    let cranker = Keypair::new();
    let mint_kp = Keypair::new();
    let validator = ER_VALIDATOR_IDENTITY;
    let mint = mint_kp.pubkey();
    let queue = find_transfer_queue(&mint, &validator).0;

    airdrop(base, &payer.pubkey(), 100 * LAMPORTS_PER_SOL)?;
    airdrop(base, &helper.pubkey(), 10 * LAMPORTS_PER_SOL)?;
    airdrop(base, &cranker.pubkey(), 10 * LAMPORTS_PER_SOL)?;
    create_mint(base, &payer, &mint_kp, decimals).context("create the mint")?;

    // The validator's fee vault pays for the work the rollup schedules —
    // including landing settlement intents back on the base. A fresh validator
    // has it empty, and an empty vault means intents are never paid for.
    airdrop(
        base,
        &magic_fee_vault_pda_from_validator(&validator),
        10 * LAMPORTS_PER_SOL,
    )?;

    // The rent PDA sponsors the destination ATA that settlement creates. It is a
    // global singleton, so on a shared validator this mostly just tops it up.
    ensure_rent_pda_funded(base, &payer)?;

    // The global vault is the pot deposits land in and settlement pays out of.
    send(
        base,
        &[InitializeGlobalVaultBuilder {
            payer: payer.pubkey(),
            mint,
        }
        .instruction()],
        &payer.pubkey(),
        &[&payer],
    )
    .context("initialize the global vault")?;

    // Fund the sender so there is something real to move end to end.
    let minted = 1_000 * 10u64.pow(decimals as u32);
    // A tenth moves into the eATA below, which is a *separate*, rollup-side
    // balance — so what stays on the base is what the sender can still spend
    // there, and that is what `initial_balance` means to the tests.
    let eata_deposit = minted / 10;
    let initial_balance = minted - eata_deposit;
    let sender_ata =
        create_ata_and_mint(base, &payer, &mint, &payer.pubkey(), minted).context("fund the sender's token account")?;

    // Before the payer is delegated, while it can still pay its own fees.
    send(
        base,
        &setup_ephemeral_ata_ixs(&payer.pubkey(), &payer.pubkey(), &mint, &validator, eata_deposit),
        &payer.pubkey(),
        &[&payer],
    )
    .context("give the sender a delegated ephemeral ATA")?;

    // `InitializeTransferQueue` also creates the queue's ephemeral ATA and vault
    // ATA, and delegates the ATA to the rollup.
    send(
        base,
        &[InitializeTransferQueueBuilder {
            payer: payer.pubkey(),
            mint,
            validator,
            requested_items: None,
        }
        .instruction()],
        &payer.pubkey(),
        &[&payer],
    )
    .context("initialize the transfer queue")?;

    // The queue sponsors the group receipt. `DepositAndQueueTransfer` creates
    // that receipt as an *ephemeral account*, and the magic program debits the
    // rent for it from the sponsor — which is the queue PDA. Rent-exemption
    // alone is not enough: those lamports back the queue's own data, and
    // spending them would leave it under-funded, so the sponsor needs a surplus
    // on top. This has to happen before delegation, while the queue's lamports
    // can still be changed on the base.
    send(
        base,
        &[system_instruction::transfer(
            &payer.pubkey(),
            &queue,
            QUEUE_SPONSOR_LAMPORTS,
        )],
        &payer.pubkey(),
        &[&payer],
    )
    .context("fund the queue so it can sponsor ephemeral accounts")?;

    // `DelegateTransferQueue` hands the queue account itself to the rollup, so
    // the crank can mutate it there.
    send(
        base,
        &[DelegateTransferQueueBuilder {
            payer: payer.pubkey(),
            queue,
            mint,
        }
        .instruction()],
        &payer.pubkey(),
        &[&payer],
    )
    .context("delegate the queue to the rollup")?;

    delegate_wallet(base, &payer, &helper).context("delegate the payer wallet")?;
    // The cranker is credited each trigger's reward, which the rollup only
    // permits for a delegated account.
    delegate_wallet(base, &cranker, &helper).context("delegate the cranker wallet")?;

    // The rollup clones the queue lazily; wait until it can serve it.
    wait_for(Duration::from_secs(30), "the queue to be cloned on the rollup", || {
        account_data(er, &queue).filter(|d| !d.is_empty())
    })?;

    let vault = GlobalVault::find_pda(&mint).0;
    let recipient = Keypair::new().pubkey();
    let recipient_ata = find_vault_ata(&mint, &recipient);

    Ok(QueueFixture {
        payer,
        helper,
        cranker,
        mint,
        validator,
        queue,
        vault,
        vault_ata: find_vault_ata(&mint, &vault),
        sender_ata,
        recipient,
        recipient_ata,
        initial_balance,
    })
}

/// Lamports to leave on a queue's crank so it can pay its own execution rewards.
/// At the ephemeral reward of 100 lamports this covers 10 000 executions.
const CRANK_FUNDING_LAMPORTS: u64 = 1_000_000;

/// Top the queue's crank up so it can pay its own execution rewards — and a
/// no-op where there is no crank to top up.
///
/// The two rollups differ here, and the test has to work against both:
///
///  * `ephemeral-validator` executes the scheduled task itself and never creates
///    a crank account, so there is nothing to fund and none ever appears;
///  * `magicblock-validator` routes tasks through Hydra, which debits each
///    execution's reward from the crank account. A crank left at zero lamports
///    is scheduled, due, and permanently stuck at `executed = 0` — with no error
///    surfaced anywhere near the queue.
///
/// Waiting briefly is what separates the two: the scheduler creates the crank a
/// slot or two after `EnsureTransferQueueCrank`, so "not there yet" and "never
/// coming" look identical for the first moment.
pub fn fund_queue_crank(er: &RpcClient, payer: &Keypair, queue: &Pubkey) -> Result<()> {
    let Some((crank, balance)) = wait_for(Duration::from_secs(5), "the queue's crank", || {
        find_queue_crank(er, queue).ok().flatten()
    })
    .ok() else {
        return Ok(());
    };
    if balance >= CRANK_FUNDING_LAMPORTS {
        return Ok(());
    }

    send(
        er,
        &[system_instruction::transfer(
            &payer.pubkey(),
            &crank,
            CRANK_FUNDING_LAMPORTS - balance,
        )],
        &payer.pubkey(),
        &[payer],
    )
    .with_context(|| format!("fund crank {crank}"))
}

/// The crank account the rollup keeps for `queue`, with its balance.
///
/// Its address is not derivable from anything we control — the rollup chooses
/// the seed, and the SDK's `find_hydra_crank_pda` covers a different scheduler
/// path — so it is found by scanning the crank program's accounts for one that
/// references the queue. Only the address is needed, so the account is never
/// decoded.
fn find_queue_crank(er: &RpcClient, queue: &Pubkey) -> Result<Option<(Pubkey, u64)>> {
    let needle = queue.to_bytes();
    Ok(er
        .get_program_accounts(&HYDRA_EPHEMERAL_PROGRAM_ID)
        .context("list hydra accounts on the rollup")?
        .into_iter()
        .find(|(_, account)| account.data.windows(32).any(|w| w == needle))
        .map(|(address, account)| (address, account.lamports)))
}

/// Read and decode a transfer queue's header from `rpc`.
pub fn queue_header(rpc: &RpcClient, queue: &Pubkey) -> Result<TransferQueueHeader> {
    let data = account_data(rpc, queue).with_context(|| format!("queue {queue} does not exist"))?;
    if data.len() < HEADER_LEN {
        bail!("queue {queue} is only {} bytes, need {HEADER_LEN}", data.len());
    }
    // The queue account is not guaranteed to be aligned for `TransferQueueHeader`.
    Ok(unsafe { core::ptr::read_unaligned(data.as_ptr() as *const TransferQueueHeader) })
}

/// Assert an SPL token account's balance, for tests that settle transfers.
pub fn token_balance(rpc: &RpcClient, token_account: &Pubkey) -> Result<u64> {
    let Some(data) = crate::rpc::account_data(rpc, token_account) else {
        bail!("token account {token_account} does not exist");
    };
    let account = spl_token_interface::state::Account::unpack(&data).context("unpack token account")?;
    Ok(account.amount)
}
