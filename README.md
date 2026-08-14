# Ephemeral SPL Token

Ephemeral SPL Token program implementing [MIMD 0013](https://github.com/magicblock-labs/magicblock-validator/discussions/550). It provides temporary (ephemeral) SPL balances that can be delegated to Data Layer Programs (DLPs), queued for delayed settlement, or routed through shuttle-based flows.

## Repository layout
- `e-token` — On-chain program (cdylib) implementing the Ephemeral SPL Token logic.
- `e-token-api` — `no_std` rlib with the program ID, instruction discriminators, and shared types used by clients and tests.
- `idl/ephemeral_spl_token.json` — Machine-readable instruction/account summary aligned with the on-chain processors.

## Instruction set
Public instructions currently handled by the program are:

Core balance management
- `0` `InitializeEphemeralAta` — idempotently create the Ephemeral ATA PDA derived from `[user, mint]`.
- `1` `InitializeGlobalVault` — create the per-mint global vault PDA derived from `[mint]`, plus the vault-owned Ephemeral ATA and vault ATA.
- `2` `DepositSplTokens` — transfer tokens into the global vault and credit a standard or shuttle Ephemeral ATA.
- `3` `WithdrawSplTokens` — transfer tokens out of the global vault and debit the caller's Ephemeral ATA.
- `10` `CloseEphemeralAta` — close an empty Ephemeral ATA and refund its rent.

Delegation and permissions
- `4` `DelegateEphemeralAta` — delegate an Ephemeral ATA to the delegation program. Raw data is empty or an appended 32-byte validator pubkey.
- `5` `UndelegateEphemeralAta` — commit and undelegate a user's delegated ATA flow.
- `6` `CreateEphemeralAtaPermission` — create the ACL permission PDA for an Ephemeral ATA. Raw data is a single permission-flags byte.
- `7` `DelegateEphemeralAtaPermission` — delegate the permission PDA associated with an Ephemeral ATA.
- `8` `UndelegateEphemeralAtaPermission` — commit and undelegate the permission PDA.
- `9` `ResetEphemeralAtaPermission` — reset permission members to the owner plus the requested ACL flags. Raw data is a single permission-flags byte.

Shuttle flows
- `11` `InitializeShuttleEphemeralAta` — initialize shuttle metadata, its shuttle Ephemeral ATA, and the shuttle wallet ATA for `[owner, mint, shuttle_id]`.
- `13` `DelegateShuttleEphemeralAta` — delegate a shuttle Ephemeral ATA. Raw data is empty or an appended 32-byte validator pubkey.
- `14` `UndelegateShuttleEphemeralAta` — commit and undelegate a shuttle wallet ATA, then schedule shuttle close/refund handling. Raw data is empty or a single escrow-index byte.
- `15` `MergeShuttleIntoEphemeralAta` — transfer the entire shuttle wallet ATA balance into a destination token account.
- `24` `SetupAndDelegateShuttleEphemeralAtaWithMerge` — initialize shuttle accounts if needed, deposit into the vault, sponsor delegation from the rent PDA, then schedule merge plus cleanup. Raw data is `shuttle_id:u32`, `amount:u64`, and an optional trailing 32-byte validator pubkey.
- `25` `DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer` — same sponsored shuttle setup/deposit/delegate flow as `24`, but schedules a private queued transfer after merging back into the owner's source token account. Raw data starts with `shuttle_id:u32` and `amount:u64`, then carries three single-byte-length-prefixed buffers: validator bytes, encrypted destination bytes, and encrypted queue-suffix bytes.
- `26` `WithdrawThroughDelegatedShuttleWithMerge` — sponsor shuttle delegation, then schedule an owner-to-shuttle transfer followed by shuttle undelegation and cleanup. Raw data is `shuttle_id:u32`, `amount:u64`, and an optional trailing 32-byte validator pubkey.

Transfer queue and automation
- `12` `InitializeTransferQueue` — create the per-mint transfer queue PDA derived from `["queue", mint]`. Raw data may be omitted, or may contain `size_bytes:u32`; `0` selects the default queue size.
- `16` `DepositAndQueueTransfer` — deposit into the vault and enqueue one or more delayed transfers. Raw data is `amount:u64`, `min_delay_ms:u64`, `max_delay_ms:u64`, `split:u32`, with an optional trailing flags byte.
- `17` `EnsureTransferQueueCrank` — ensure the recurring transfer-queue crank is scheduled.
- `19` `DelegateTransferQueue` — delegate the per-mint transfer queue PDA.
- `20` `SponsoredLamportsTransfer` — transfer lamports via the sponsored rent mechanism.

Rent PDA
- `23` `InitializeRentPda` — initialize the global rent-sponsoring PDA derived from `["rent"]`.

Internal automation instructions
- `196` `UndelegationCallback` — delegation-program callback used to restore delegated account state.
- `197` `CloseShuttleAtaIntent` — internal close/refund handler for shuttle flows.
- `198` `ExecuteReadyQueuedTransfer` — internal settlement handler for a ready queued transfer.
- `199` `ProcessTransferQueueTick` — internal recurring crank callback that checks a queue and schedules settlement.
- `201` `UndelegateWithdrawAndCloseShuttleEphemeralAta` — internal post-delegation shuttle withdrawal and close/refund handler.

Discriminators `18`, `21`, and `22` are currently unused.

Program ID and external program
- The Ephemeral SPL Token program ID is declared in `e-token-api/src/lib.rs` under `program::id_address()`.
- Delegation, undelegation, magic scheduling, and ACL flows are implemented through `ephemeral-rollups-pinocchio`.

## Prerequisites
- Rust (toolchain pinned via `rust-toolchain.toml`).
- Solana CLI = `2.3.4` (see `[workspace.metadata.cli]` in `Cargo.toml`).

## Build
Build the on-chain program to SBF:

```bash
cargo build-sbf
```

## Tests
Run the test suite with program logs enabled:

```bash
cargo test-sbf --features logging
```

You can run a single test by passing its name, for example:

```bash
cargo test-sbf --features logging delegate_ephemeral_ata
```

Tests live under `e-token/tests/` and cover balance accounting, delegation/undelegation, shuttle flows, permissions, the rent PDA and lamports-PDA flows, and transfer-queue automation.

### End-to-end tests (live validators)

`e-token-e2e` runs against the real two-validator stack instead of an in-process
bank. This is the only place the transfer-queue crank is genuinely exercised.
The program asks for the crank through the magic program's `ScheduleTask`, and
the in-process suite can only check that *request* — its magic mock captures the
call and never executes it. Whether a scheduled task is then actually fired, on
cadence, forever, is the rollup's half of the contract, and that is the half now
running on Hydra. Only a live rollup tests it.

Install the validators once, then run the suite:

```bash
npm i -g @magicblock-labs/ephemeral-validator   # mb-test-validator + ephemeral-validator
make test-e2e
```

`make test-e2e` builds the program (with `logging`, so a failing transaction
reports which check rejected it), starts both validators, runs the tests, and
stops the validators again.

Starting a base validator dominates the runtime, so for a tight edit/test loop
leave one running and reuse it:

```bash
make e2e-base-validator                # terminal 1: base validator, stays up
make test-e2e SKIP_BASE_VALIDATOR=1    # terminal 2: attaches to it, repeatedly
```

`SKIP_ER_VALIDATOR=1` (with `make e2e-er-validator`) does the same for the
rollup, and the two combine. A skipped validator is attached to and left running
afterwards; a validator that is *not* skipped refuses to start if its port is
already busy, rather than silently reusing a stale instance. `make e2e-stop`
cleans up anything an interrupted run orphaned, and `KEEP_E2E_LOGS=1` (with
`E2E_ER_LOG=info`) retains the temporary ledgers and validator logs for
post-mortem.

What the suite runs is the full private-transfer flow — one base-layer
instruction that deposits, delegates a shuttle whose post-action queues the
transfer on the rollup, and lets the crank settle it back — asserting on the
recipient's base-layer balance at the end.

The validator pair is all it needs. `ephemeral-validator` executes the scheduled
task itself, so no external cranker has to be running. A `magicblock-validator`
rollup instead routes tasks through Hydra, which debits each execution's reward
from a crank account; the suite tops that account up itself, and is a no-op on a
rollup that has no crank to top up (see `fund_queue_crank` in
`e-token-e2e/src/fixture.rs`).

## Notes
- The workspace depends on `ephemeral-rollups-pinocchio` and several Solana crates; ensure your local environment matches the versions declared in the workspace `Cargo.toml`.
- The program enables additional logs when compiled with the `logging` feature, which is useful for debugging unit and integration tests.
