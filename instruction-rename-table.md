# ESplInstruction Rename Table

Working table for shortening mechanism-heavy instruction names into outcome-first public names.

## Naming taxonomy

Use terms consistently so callers can infer whether an instruction completes the
user-visible effect immediately or starts background work.

| Term         | Meaning                                                                                  |
|--------------|------------------------------------------------------------------------------------------|
| `Transfer`  | The instruction performs the token/lamports movement in the same transaction.            |
| `Queue`     | The instruction writes work into a durable transfer queue for later settlement.           |
| `Schedule`  | The instruction registers a future/cranked/delegated action outside the transfer queue.   |
| `Prepare`   | The instruction creates, validates, or delegates readiness state for later use.           |
| `Finalize`  | The instruction completes settlement, cleanup, or closure for prior async state.          |

`Async` is the caller-facing category for work completed later in the background.
Use docs to spell out whether the async path is backed by a transfer queue,
Hydra schedule, delegated post-action, or another mechanism.

## Rename implementation notes

- Treat `TokenAccount`, `EphemeralAta`, and shuttle accounts as protocol
  building blocks. Prefer primitive operation names over names that spell out
  every account transition.
- `Sweep` means moving the full available balance out of an intermediate or
  temporary account into a destination account. It is appropriate for primitive
  operations like moving the full shuttle-held balance into a destination token
  account.
- When applying final names in `ESplInstruction` docs, add the temporary
  migration line as the final doc-comment line immediately above the renamed
  variant: `Old Name: <old variant name>`. This fixed location is only for
  mental mapping during the rename and can be removed after the transition.

| Ix | Current Name                                                          | Working Candidate               | Final Name                      | Notes                                                                                                   |
|---:|-----------------------------------------------------------------------|---------------------------------|---------------------------------|---------------------------------------------------------------------------------------------------------|
| 11 | `InitializeShuttleEphemeralAta`                                       | `InitializeShuttle`             | `InitializeShuttle`             | Hides metadata/EATA/wallet setup behind the shuttle domain object.                                      |
| 13 | `DelegateShuttleEphemeralAta`                                         | `DelegateShuttle`               | `DelegateShuttle`               | Shorter and keeps delegation as the high-level operation.                                               |
| 14 | `UndelegateAndCloseShuttleToOwner`                                    | `StartAsyncShuttleClose`        | `StartAsyncShuttleClose`        | Starts the async shuttle-close flow; docs should note undelegate/settle/refund happen later.            |
| 15 | `MergeShuttleIntoEphemeralAta`                                        | `SweepShuttleBalance`           | `SweepShuttleBalance`           | Primitive sweep: move the full shuttle balance into a destination token account.                        |
| 20 | `SponsoredLamportsTransfer`                                           | `StartAsyncLamportsTransfer`    | `StartAsyncLamportsTransfer`    | Starts the async lamports-transfer flow; docs should note rent sponsorship and delegated PDA mechanics. |
| 24 | `SetupAndDelegateShuttleEphemeralAtaWithMerge`                        | `StartAsyncShuttleTransfer`     | `StartAsyncShuttleTransfer`     | Starts the async shuttle-transfer flow; docs should note completion happens via delegated post-actions. |
| 25 | `DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer`    | `StartAsyncPrivateTransfer`     | `StartAsyncPrivateTransfer`     | Starts the async private-transfer flow; docs should note settlement is queue-backed.                    |
| 26 | `WithdrawThroughDelegatedShuttleWithMerge`                            | `StartAsyncShuttleWithdraw`     | `StartAsyncShuttleWithdraw`     | Starts the async shuttle-withdraw flow; docs should note transfer/cleanup happen as delegated actions.  |
| 28 | `ExecutePendingTransferQueueRefill`                                   | `StartAsyncTransferQueueRefill` | `StartAsyncTransferQueueRefill` | Starts the async transfer-queue refill flow; docs should note refill is delegated via lamports PDA.     |

## PR Description Bullets

1. **InitializeShuttleEphemeralAta**
   - New name: **InitializeShuttle**
   - Rationale: Treats shuttle as the protocol primitive and moves metadata/EATA/wallet ATA details into docs.
2. **DelegateShuttleEphemeralAta**
   - New name: **DelegateShuttle**
   - Rationale: Keeps delegation as the direct operation while hiding the shuttle EATA implementation detail.
3. **UndelegateAndCloseShuttleToOwner**
   - New name: **StartAsyncShuttleClose**
   - Rationale: Names the caller-facing effect: starting a close flow whose undelegate/settle/refund work completes asynchronously.
   - The old name implies the intended close/refund effect is produced when the transaction is finalized, but the actual effect completes later in the background, possibly well after this transaction finalizes.
4. **MergeShuttleIntoEphemeralAta**
   - New name: **SweepShuttleBalance**
   - Rationale: Describes the primitive operation: move the full shuttle-held balance into the destination token account.
5. **SponsoredLamportsTransfer**
   - New name: **StartAsyncLamportsTransfer**
   - Rationale: Clarifies that the instruction starts a background lamports-transfer flow via sponsored/delegated PDA mechanics.
6. **SetupAndDelegateShuttleEphemeralAtaWithMerge**
   - New name: **StartAsyncShuttleTransfer**
   - Rationale: Names the high-level async transfer flow instead of listing setup, delegation, and merge mechanics.
7. **DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer**
   - New name: **StartAsyncPrivateTransfer**
   - Rationale: Names the user-facing private transfer flow and leaves queue-backed settlement details in docs.
8. **WithdrawThroughDelegatedShuttleWithMerge**
   - New name: **StartAsyncShuttleWithdraw**
   - Rationale: Names the high-level async withdraw flow instead of encoding delegated shuttle and merge steps in the variant.
9. **ExecutePendingTransferQueueRefill**
   - New name: **StartAsyncTransferQueueRefill**
   - Rationale: Clarifies that the instruction starts the async queue-refill/top-up path rather than completing all refill work inline.
