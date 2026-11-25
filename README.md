<h1 align="center">Ephemeral SPL Token</h1>

<p align="center">
  <b>SPL token flows between Solana and MagicBlock Ephemeral Rollups.</b>
</p>

<p align="center">
  <a href="#-capabilities">✨ Capabilities</a> ·
  <a href="#-how-it-fits-together">🌉 Design</a> ·
  <a href="#-get-started">🚀 Get started</a> ·
  <a href="#-repository-map">🧩 Repository</a>
</p>

---

Ephemeral SPL Token lets applications deposit SPL tokens into MagicBlock
Ephemeral Rollups (ERs), use their balances there, and withdraw to Solana.
Private payments carry encrypted recipient and timing data into the rollup
before queuing delivery on the base chain.

This repository contains the on-chain program, its Rust instruction and account
types, a transaction-building HTTP API, and local integration tooling.

## ✨ Capabilities

| Capability | What it enables |
| :-- | :-- |
| **Deposit and withdraw** | Move tokens through a per-mint vault that backs ephemeral balances. |
| **Delegated balances** | Use balances in the rollup, then commit state and return control to Solana. |
| **Private transfers** | Encrypt recipient and timing parameters; settle delayed or split payments through a validator-specific queue. |
| **Temporary shuttles** | Move funds across the delegation boundary with sponsored setup and automated follow-up actions. |
| **Permissions and stealth pools** | Control account access and use handles to address payments. |
| **Application integration** | Build transactions through the [HTTP API](api/README.md) or accept [delegated-USDC payments](api/PAYMENTS.md). |

## Instruction catalog

### Shuttle flows

- `11` `InitializeShuttle` — initialize or validate shuttle metadata, its shuttle Ephemeral ATA, and the shuttle wallet ATA for `[owner, mint, shuttle_id]`.
- `13` `DelegateShuttle` — delegate a shuttle Ephemeral ATA.
- `14` `AsyncShuttleClose` — undelegate a shuttle and schedule close/refund handling.
- `15` `SweepShuttleBalance` — transfer the entire shuttle wallet ATA balance into a destination token account.
- `24` `AsyncShuttleTransfer` — set up, deposit into, and delegate a shuttle, then schedule merge and cleanup.
- `25` `AsyncPrivateTransfer` — start a queue-backed private transfer that settles to the recipient later.
- `26` `AsyncShuttleWithdraw` — delegate a shuttle, then schedule the owner-to-shuttle transfer and cleanup.
- `34` `AsyncPrivateTransferToEncryptedDestination` — start a private transfer whose destination remains encrypted on base.
- `35` `SweepShuttleBalanceToMagicAta` — create the destination Magic ATA and sweep the full shuttle balance into it.
- `36` `EnsureMagicAtaDestination` — create a destination Magic ATA before a same-transaction SPL transfer.
- `37` `AsyncShuttleWithdrawWithFee` — start the instruction 26 withdrawal with a relay fee routed to the transfer queue.

### Transfer queue and automation

- `12` `InitializeTransferQueue` — create the per-mint transfer queue PDA.
- `16` `DepositAndQueueTransfer` — deposit into the vault and enqueue delayed transfers.
- `17` `EnsureTransferQueueCrank` — ensure recurring transfer-queue processing is scheduled.
- `19` `DelegateTransferQueue` — delegate the per-mint transfer queue PDA.
- `20` `AsyncLamportsTransfer` — start a sponsored lamports transfer with scheduled cleanup.
- `28` `AsyncTransferQueueRefill` — start the async transfer-queue refill path.

## 🌉 How it fits together

**Vault → Delegated execution → Settlement**

On Solana, a global vault holds tokens for each mint. An ephemeral associated
token account (eATA) records an owner's balance; it is program state, not an
ordinary SPL token account. Delegation makes that state available to the rollup.
Shuttles provide temporary accounts for moving funds across that boundary without
requiring a long-lived delegated balance for every flow.

Queued private payments follow three stages:

1. **Stage funds.** The sender starts a shuttle flow with encrypted recipient
   and timing parameters.
2. **Queue in the rollup.** Post-delegation actions enqueue the payment and
   clean up the shuttle.
3. **Settle on Solana.** Queue automation releases ready transfers to recipients.

The queue for the selected mint and validator must already exist and be delegated.
**The initial transaction does not complete recipient settlement.**

The token program owns vault accounting and transfer orchestration. The
delegation, Magic, ACL, and scheduling programs supply the surrounding execution
and automation services. See the [program guide](program/README.md) for account
roles and source entry points.

## 🚀 Get started

### Integrate an application

Start with the [HTTP API guide](api/README.md) for deposits, withdrawals,
transfers, authentication, and local API setup. Transaction-building endpoints
return serialized transactions for the client to complete, sign, and submit;
sponsored transactions may already contain a sponsor signature.

For merchant flows, see [payment checkouts](api/PAYMENTS.md).

### Work on the program

Install the Rust toolchain pinned in [rust-toolchain.toml](rust-toolchain.toml)
and Solana CLI/SBF tooling, then build and run the in-process suite:

```bash
make build
make test
```

See the [development guide](docs/development.md) for prerequisites,
live-validator testing, and compute-unit comparisons. Maintainers deploy through
the [Squads upgrade workflow](docs/upgrades.md).

## 🧩 Repository map

| Area | Responsibility |
| :-- | :-- |
| [`program`](program/README.md) | Pinocchio-based on-chain program: vaults, delegation, shuttles, and queued settlement. |
| [`interface`](interface/src/lib.rs) | `no_std` Rust API: program ID, instruction data, account layouts, and PDA derivation. |
| [`api`](api/README.md) | Cloudflare Worker HTTP API for transaction building and payment integration. |
| [`program/tests`](program/tests) | In-process program tests and external-program mocks. |
| [`e-token-e2e`](e-token-e2e) | Live base-validator and rollup tests for private-transfer settlement. |
| [`compare-metrics`](compare-metrics) | Compare compute-unit measurements collected by program tests. |
