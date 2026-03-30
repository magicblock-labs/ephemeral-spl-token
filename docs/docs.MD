## Private Payments

`process_deposit_and_delegate_shuttle_ephemeral_ata_with_merge_and_private_transfer`
(instruction `25`) does not directly deliver funds to the recipient.
It stages a temporary sponsored shuttle flow, injects an encrypted
`DEPOSIT_AND_QUEUE_TRANSFER` as a post-delegation action, and only later settles
to the recipient through the validator-scoped transfer queue.

Important preconditions:
- The transfer queue PDA for `[QUEUE_SEED, mint, validator]` must already exist.
- That queue must already be delegated. Instruction `25` only validates and uses it.
- Automatic delivery happens later through queue-crank automation, not inside the
  outer transaction itself.

### Technical Sequence

```mermaid
sequenceDiagram
    autonumber
    participant U as User / payer + owner
    participant ET as e-token program
    participant RP as rent_pda
    participant GV as Global vault + vault ATA
    participant ER as Delegated execution
    participant Q as Delegated transfer queue
    participant MG as Magic / crank automation
    participant R as Recipient ATA

    Note over U,Q: Prerequisite: queue PDA for [QUEUE_SEED, mint, validator] already exists, is delegated, and is normally crank-enabled.

    U->>ET: ix 25 DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer<br/>args = shuttle_id, amount, validator?, encrypted destination, encrypted delay/split suffix
    ET->>ET: Parse args<br/>Resolve validator (explicit or DEFAULT_VALIDATOR_IDENTITY)<br/>Validate queue PDA seeds and require queue owner = delegation program
    ET->>RP: Charge sponsored shuttle setup fee
    ET->>ET: initialize_shuttle_ephemeral_ata_with_sponsor()<br/>Create shuttle metadata + shuttle EATA + shuttle wallet ATA if needed
    ET->>GV: transfer_to_vault_for_mint()<br/>owner_source_token -> vault_token (amount)
    ET->>ET: shuttle_eata.amount += amount
    ET->>ER: delegate_sponsored_shuttle_with_post_actions()<br/>post actions = [1 merge back, 2 encrypted depositAndQueueTransfer, 3 undelegate+close]

    ER->>ET: Action 1<br/>MERGE_SHUTTLE_INTO_EPHEMERAL_ATA(owner_source_token)
    ET->>U: shuttle_wallet_ata -> owner_source_token

    ER->>ER: Decrypt destination payload<br/>Decrypt queue suffix (min_delay_ms, max_delay_ms, split, flags)
    ER->>ET: Action 2<br/>DEPOSIT_AND_QUEUE_TRANSFER(queue, vault, mint,<br/>owner_source, vault_token, decrypted destination ref,<br/>owner signer, token program, shuttle_wallet_ata)
    ET->>GV: owner_source_token -> vault_token (amount)
    ET->>Q: Enqueue queued transfer items<br/>Store destination owner + ready_at + flags per split

    ER->>ET: Action 3<br/>UNDELEGATE_SHUTTLE_EPHEMERAL_ATA
    ET->>MG: commit_and_undelegate()<br/>schedule CLOSE_SHUTTLE_ATA_INTENT
    MG->>ET: CLOSE_SHUTTLE_ATA_INTENT
    ET->>RP: Refund shuttle rents and close temporary shuttle accounts

    Note over MG,Q: Final delivery is a separate delayed path driven by queue automation.

    loop each ready queued split
        MG->>ET: PROCESS_TRANSFER_QUEUE_TICK
        ET->>Q: Peek ready item<br/>Build EXECUTE_READY_QUEUED_TRANSFER<br/>Pop item from queue
        MG->>ET: EXECUTE_READY_QUEUED_TRANSFER
        opt QUEUED_TRANSFER_FLAG_CREATE_IDEMPOTENT_ATA set
            ET->>RP: Sponsor recipient ATA creation
            ET->>R: Create ATA idempotently for destination owner
        end
        ET->>R: vault_token -> recipient ATA
    end
```

### User View

```mermaid
flowchart TD
    P0[Precondition<br/>A validator-specific queue for this mint already exists and is delegated] --> A[User signs one base transaction<br/>instruction 25 + amount + optional validator + encrypted recipient + encrypted delay plan]

    subgraph Immediate base transaction
        A --> B[Program sponsors a temporary shuttle flow]
        B --> C[Amount is moved into the vault-backed shuttle state]
        C --> D[Delegated execution runs 3 actions in order]
        D --> D1[1. Merge shuttle balance back to the sender source ATA]
        D1 --> D2[2. Re-deposit the same amount into the delegated transfer queue<br/>using the encrypted recipient and split/delay policy]
        D2 --> D3[3. Undelegate and close the temporary shuttle<br/>refund its rent to rent_pda]
    end

    D3 --> E[Recipient still has not been paid yet]

    subgraph Later automated settlement
        E --> F[Queue automation waits until each split is ready]
        F --> G[Each ready split is released from the global vault]
        G --> H[Recipient ATA is created from rent_pda only if the queued flags require it]
        H --> I[Recipient receives one or more splits on base]
    end

    I --> J[Result<br/>Base-to-base private transfer with delayed settlement through the validator queue]
```

### What This Means

- The shuttle is temporary infrastructure, not the final recipient path.
- The destination is not paid in the outer instruction `25`.
- Privacy comes from encrypting the destination and queue policy inside the
  delegated action payload.
- Final delivery happens later from the global vault through the queue, possibly
  split across multiple releases.
