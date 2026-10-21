# Ephemeral SPL Token program

The on-chain program manages vault-backed ephemeral balances and coordinates
transfers between Solana and MagicBlock Ephemeral Rollups. It is built with
Pinocchio and exposes its shared Rust types through `ephemeral-spl-api` in
[`e-token-api`](../e-token-api/src/lib.rs).

**Program ID** · `SPLxh1LVZzEkX99H6rqYizhytLWPZVV296zyYDPagv2`

Declared in [`e-token-api/src/lib.rs`](../e-token-api/src/lib.rs).

## Account model

| Account | Role |
| :-- | :-- |
| [Global vault](../e-token-api/src/state/global_vault.rs) | Per-mint PDA identifying the token account that holds vault funds. |
| [Ephemeral ATA](../e-token-api/src/state/ephemeral_ata.rs) | Owner-and-mint PDA recording an ephemeral balance; distinct from an SPL token account. |
| [Shuttle](../e-token-api/src/state/shuttle_ephemeral_ata.rs) | Temporary state for an owner, mint, and shuttle ID; used with an eATA and wallet ATA to move funds across the delegation boundary. |
| [Transfer queue](../e-token-api/src/state/transfer_queue.rs) | Mint-and-validator PDA holding delayed transfers for automated settlement. |
| [Stealth pool](../e-token-api/src/state/stealth_pool.rs) | Handle-derived destination state whose keys are managed in delegated execution. |
| [Rent PDA](src/processor/internal/rent_pda.rs) | Shared sponsor for account setup and automated flows; receives sponsored rent refunds during cleanup. |

## Transfer lifecycle

### Deposits and withdrawals

A deposit transfers SPL tokens into the vault and credits a program-owned eATA.
A withdrawal debits that balance and releases tokens from the vault. Delegation
makes ephemeral state available for rollup execution; committing and undelegating
returns control to the base chain.

### Shuttle flows

Shuttle flows combine setup, vault deposits or withdrawals, delegation, and
post-delegation actions. A merge moves shuttle wallet tokens into the destination
token account; cleanup and base-chain settlement are separate actions.

### Queued private payments

Encrypted destination and timing parameters pass into delegated execution, where
the payment is queued as one or more transfers. Scheduled queue processing later
triggers delivery on Solana. The selected mint-and-validator queue must already
exist and be delegated. Shuttle cleanup and recipient delivery are separate
completion points.

## Instructions and implementation

| Reference | Where to look |
| :-- | :-- |
| Public instruction discriminators | [Instruction enum](../e-token-api/src/instruction.rs) |
| Argument layouts | [Instruction types](../e-token-api/src/instructions) |
| Account layouts and PDA derivation | [State types](../e-token-api/src/state) |
| Instruction routing and execution | [Dispatcher](src/instruction.rs) and [processors](src/processor) |

The processors coordinate with the delegation, ACL, Magic, and scheduling
programs for account control, permissions, and automated actions.

## Development

See [build and test instructions](../docs/development.md) and the
[program upgrade procedure](../docs/upgrades.md). For application-facing
transaction construction, start with the [HTTP API](../api/README.md).
