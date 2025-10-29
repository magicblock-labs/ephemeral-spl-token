# Ephemeral SPL Token

Ephemeral SPL Token program implementing [MIMD 0013](https://github.com/magicblock-labs/magicblock-validator/discussions/550). It provides temporary (ephemeral) balances for SPL tokens that can be delegated to Data Layer Programs (DLPs) and later undelegated with state reconciliation.

## Repository layout
- `e-token` — On-chain program (cdylib) implementing the Ephemeral SPL Token logic.
- `e-token-api` — `no_std` rlib with the program ID, instruction discriminators, and shared types used by clients and tests.

## Key functionalities
The program exposes the following instructions (see `e-token-api/src/lib.rs`):
- `0` InitializeEphemeralAta — create the Ephemeral ATA PDA derived from `[payer, mint]`.
- `1` InitializeGlobalVault — create the global vault PDA derived from `[mint]`.
- `2` DepositSplTokens — transfer tokens from the user into the global vault and increase the Ephemeral ATA balance.
- `3` WithdrawSplTokens — transfer tokens back to the user from the global vault and decrease the Ephemeral ATA balance.
- `4` DelegateEphemeralAta — delegate the Ephemeral ATA to a DLP program using PDA seeds.
- `5` UndelegateEphemeralAta — commit state and undelegate via the delegation program.

Program ID and external program:
- Ephemeral SPL Token Program ID is declared in `e-token-api/src/lib.rs` under `program::id()`.
- Uses the MagicBlock Delegation Program via `ephemeral-rollups-pinocchio` for delegation/undelegation flows.

## Prerequisites
- Rust (toolchain pinned via `rust-toolchain.toml`).
- Solana CLI = `2.3.4` (see `[workspace.metadata.cli]` in `Cargo.toml`).

## Build
Build the on-chain program to SBF:

```bash
cargo build-sbf
```

## Tests
Run the test suite (with program logs enabled via the `logging` feature):

```bash
cargo test-sbf --features logging
```

Tips:
- You can run a single test by passing its name, for example:
  ```bash
  cargo test-sbf --features logging delegate_ephemeral_ata
  ```
- Tests live under `e-token/tests/` and cover delegation/undelegation flows and balance accounting.

## Notes
- The workspace depends on `ephemeral-rollups-pinocchio` and several Solana crates; ensure your local environment matches the versions declared in the workspace `Cargo.toml`.
- The program enables additional logs when compiled with the `logging` feature; this is useful for debugging both unit and integration tests.

