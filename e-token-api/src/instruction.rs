use alloc::vec::Vec;

///
/// ESplInstruction defines the public instructions
///
/// Reserved values:
///     - single: 196         : for DLP undelegatation callback used to restore delegated state
///     - range: (201.. =255) : for ESPL internal instructions
///
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ESplInstruction {
    /// 0 - InitializeEphemeralAta: initialize the ephemeral ATA account derived from [user, mint]
    InitializeEphemeralAta = 0,

    /// 1 - InitializeGlobalVault: initialize the global vault [mint] PDA plus vault-owned Ephemeral ATA and vault ATA
    InitializeGlobalVault = 1,

    /// 2 - DepositSplTokens: transfer tokens to global vault and increase EphemeralAta amount
    ///     Works for both standard EATA and shuttle EATA, as long as the data account is program-owned.
    DepositSplTokens = 2,

    /// 3 - WithdrawSplTokens: transfer tokens from global vault back to user and decrease EphemeralAta amount
    WithdrawSplTokens = 3,

    /// 4 - DelegateEphemeralAta: delegate the ephemeral ATA to a DLP program using PDA seeds
    DelegateEphemeralAta = 4,

    /// 5 - UndelegateEphemeralAta: commit state and undelegate an ephemeral ATA via the delegation program
    UndelegateEphemeralAta = 5,

    /// 6 - CreateEphemeralAtaPermission: create a permission account for the ephemeral ATA
    ///     Instruction data:
    ///     [0] bump
    ///     [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    CreateEphemeralAtaPermission = 6,

    /// 7 - DelegateEphemeralAtaPermission: delegate the permission PDA for an ephemeral ATA
    DelegateEphemeralAtaPermission = 7,

    /// 8 - UndelegateEphemeralAtaPermission: commit and undelegate the permission PDA
    UndelegateEphemeralAtaPermission = 8,

    /// 9 - ResetEphemeralAtaPermission: reset permission members to creation-time defaults
    ///     Instruction data:
    ///     [0] bump
    ///     [1] MemberFlags bitfield encoded via MemberFlags::to_acl_flag_byte.
    ResetEphemeralAtaPermission = 9,

    /// 10 - CloseEphemeralAta: close an empty ephemeral ATA and refund rent to recipient
    CloseEphemeralAta = 10,

    /// 11 - InitializeShuttleEphemeralAta: initialize shuttle account derived from [owner, mint, shuttle_id]
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    bump
    InitializeShuttleEphemeralAta = 11,

    /// 12 - InitializeTransferQueue: initialize per-mint transfer queue PDA derived from [QUEUE_SEED, mint]
    ///      Instruction data:
    ///      []        default size (9728 bytes)
    ///      [0..4]    optional queue size in bytes (u32 LE), 0 => default
    InitializeTransferQueue = 12,

    /// 13 - DelegateShuttleEphemeralAta: delegate shuttle account to a DLP program using PDA seeds
    DelegateShuttleEphemeralAta = 13,

    /// 14 - UndelegateAndCloseShuttleToOwner: revoke delegation on a shuttle ATA
    ///      and schedule settlement/close using an owner-owned destination token account.
    UndelegateAndCloseShuttleToOwner = 14,

    /// 15 - MergeShuttleIntoEphemeralAta: transfer all shuttle ATA funds into destination ATA and keep shuttle account open
    MergeShuttleIntoEphemeralAta = 15,

    /// 16 - DepositAndQueueTransfer: transfer tokens from signer into the vault ATA and enqueue one or more delayed transfers
    ///      Instruction data:
    ///      [0..8]    amount (u64 LE)
    ///      [8..16]   min_delay_ms (u64 LE), 0 => immediate
    ///      [16..24]  max_delay_ms (u64 LE), must be >= min_delay_ms
    ///      [24..28]  split count (u32 LE), must be >= 1
    ///      [28]      optional legacy flags (u8)
    ///      [28..36]  optional client_ref_id (u64 LE) when flags are omitted
    ///      [29..37]  optional client_ref_id (u64 LE) after legacy flags
    DepositAndQueueTransfer = 16,

    /// 17 - EnsureTransferQueueCrank: ensure the per-mint recurring queue crank is scheduled
    ///      Instruction data:
    ///      []
    EnsureTransferQueueCrank = 17,

    /// 19 - DelegateTransferQueue: delegate the per-mint transfer queue PDA to the delegation program
    ///      Instruction data:
    ///      []        no instruction args
    DelegateTransferQueue = 19,

    /// 20 - SponsoredLamportsTransfer: create a zero-data PDA derived from
    ///      [b"lamports", payer, destination, salt], fund it with the requested
    ///      lamports plus sponsored rent from the global rent PDA, delegate it,
    ///      then schedule post-delegation transfer + cleanup actions.
    ///      Instruction data:
    ///      [0..8]   amount (u64 LE)
    ///      [8..40]  salt ([u8; 32])
    SponsoredLamportsTransfer = 20,

    /// 23 - InitializeRentPda: initialize the global rent-sponsoring PDA derived from ["rent"]
    ///      Instruction data:
    ///      []        no instruction args
    InitializeRentPda = 23,

    /// 24 - SetupAndDelegateShuttleEphemeralAtaWithMerge: initialize shuttle metadata/EATA/wallet ATA,
    ///      deposit tokens into the shuttle EATA through the global vault, sponsor delegation from
    ///      the global rent PDA, and schedule post-delegation merge + cleanup.
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    shuttle metadata bump
    ///      [5..13] deposit amount (u64 LE)
    ///      [13..45] optional validator pubkey
    SetupAndDelegateShuttleEphemeralAtaWithMerge = 24,

    /// 25 - DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer:
    ///      same setup/deposit/delegate flow as instruction 24, but instead of using instruction 24's
    ///      merge-to-destination behavior, the first post-delegation action restores the owner's
    ///      source token account by merging the shuttle balance back there, then a third post-delegation
    ///      action schedules a private transfer of the same amount to the destination owner's
    ///      canonical ATA.
    ///      SDK callers must account for that queued private transfer when calculating required source
    ///      balances and expected destination credits; the destination does not hold the final funds
    ///      immediately after the merge/cleanup steps.
    ///      Instruction data:
    ///      [0..4]   shuttle_id (u32 LE)
    ///      [4..12]  deposit amount (u64 LE)
    ///      [12..]   len-prefixed optional validator pubkey bytes
    ///      [...]    len-prefixed encrypted destination owner pubkey bytes
    ///      [...]    len-prefixed encrypted packed suffix
    ///               (min_delay_ms:u64, max_delay_ms:u64, split:u32, client_ref_id?:u64)
    ///               Legacy payloads may still append flags before client_ref_id.
    DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer = 25,

    /// 26 - WithdrawThroughDelegatedShuttleWithMerge: initialize shuttle metadata/EATA/wallet ATA,
    ///      sponsor delegation from the global rent PDA, then schedule a post-delegation transfer
    ///      from the owner ATA into the shuttle wallet ATA followed by shuttle undelegate
    ///      and close/refund.
    ///      Instruction data:
    ///      [0..4] shuttle_id (u32 LE)
    ///      [4]    shuttle metadata bump
    ///      [5..13] transfer amount (u64 LE)
    ///      [13..45] optional validator pubkey
    WithdrawThroughDelegatedShuttleWithMerge = 26,

    /// 27 - AllocateTransferQueue: allocates more space for the transfer queue
    AllocateTransferQueue = 27,

    /// 28 - ProcessPendingTransferQueueRefill: permissionless idempotent helper that
    ///      checks the queue-refill-state PDA and, when pending, tops up the queue
    ///      lamports from the global rent PDA.
    ///      Instruction data:
    ///      []        no instruction args
    ExecutePendingTransferQueueRefill = 28,

    /// 29 - SchedulePrivateTransfer: small user-signed ix that creates the
    ///      stash PDA on first use, funds it + a Hydra crank from the global
    ///      rent PDA, and CPIs into `hydra::Create` with a one-shot crank that
    ///      will fire `ExecuteScheduledPrivateTransfer` as soon as possible.
    ///      Designed to be appended to a swap tx where the swap's
    ///      `destinationTokenAccount` is the stash ATA of `(user, mint)`.
    ///      Keeps the outer-tx footprint tight: the pubkeys that ix 25 and
    ///      the timeout-refund path need at trigger time are derived on-chain
    ///      from client-supplied bumps + hard-coded program IDs, so the caller
    ///      passes only 7 accounts.
    ///      Accounts:
    ///      [0] user (signer), [1] stash_pda (w), [2] rent_pda (w),
    ///      [3] hydra_crank_pda (w), [4] hydra_program,
    ///      [5] system_program, [6] token_program.
    ///      Instruction data:
    ///      [0..4]   shuttle_id (u32 LE)
    ///      [4]      stash_pda bump
    ///      [5..37]  mint pubkey (32 B)
    ///      [37]     shuttle bump
    ///      [38]     shuttle_eata bump
    ///      [39]     shuttle_wallet_ata bump
    ///      [40]     buffer bump
    ///      [41]     delegation_record bump
    ///      [42]     delegation_metadata bump
    ///      [43]     global_vault bump
    ///      [44]     vault_token bump
    ///      [45]     stash_ata bump
    ///      [46]     queue bump
    ///      [47..]   len-prefixed optional validator pubkey (0 or 32)
    ///      [...]    len-prefixed encrypted destination owner pubkey
    ///      [...]    len-prefixed encrypted packed suffix (same format as ix 25)
    SchedulePrivateTransfer = 29,

    /// 30 - ExecuteScheduledPrivateTransfer: top-level callback fired by the
    ///      Hydra scheduler. Permissionless, no signer metas (Hydra forbids
    ///      them). Re-derives the stash PDA from [b"stash", user, mint] and
    ///      self-CPIs into instruction 25 using `invoke_signed` so the PDA
    ///      signs for both the `payer` and `owner` slots. If the callback is
    ///      triggered after the timeout window, it refunds the stash ATA balance
    ///      to the user's ATA instead.
    ///      Instruction data:
    ///      [0..32]  user pubkey (stash PDA seed)
    ///      [32]     stash PDA bump
    ///      [33..37] shuttle_id (u32 LE)
    ///      [37..]   len-prefixed optional validator pubkey
    ///      [...]    len-prefixed encrypted destination owner pubkey
    ///      [...]    len-prefixed encrypted packed suffix (same format as ix 25)
    ExecuteScheduledPrivateTransfer = 30,

    /// 31 - DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransferAndStashClose:
    ///      ix 25 + a fixed `stash_close_seeds: [user(32) | stash_bump(1)]` appended
    ///      before `encrypted_data_suffix`. Self-CPI'd by `ExecuteScheduledPrivateTransfer`.
    ///      Triggers stash ATA + stash PDA refund to the rent PDA after settlement.
    DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransferAndStashClose = 31,
}

impl ESplInstruction {
    #[inline(always)]
    pub const fn value(self) -> u8 {
        self as u8
    }

    #[inline(always)]
    pub const fn to_bytes(self) -> [u8; 1] {
        [self.value()]
    }

    #[inline(always)]
    pub fn to_vec(self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }

    #[inline(always)]
    pub fn with_data(self, instruction_data: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(1 + instruction_data.len());
        data.extend_from_slice(&self.to_bytes());
        data.extend_from_slice(instruction_data);
        data
    }
}

impl TryFrom<u8> for ESplInstruction {
    type Error = ();

    #[inline(always)]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::InitializeEphemeralAta),
            1 => Ok(Self::InitializeGlobalVault),
            2 => Ok(Self::DepositSplTokens),
            3 => Ok(Self::WithdrawSplTokens),
            4 => Ok(Self::DelegateEphemeralAta),
            5 => Ok(Self::UndelegateEphemeralAta),
            6 => Ok(Self::CreateEphemeralAtaPermission),
            7 => Ok(Self::DelegateEphemeralAtaPermission),
            8 => Ok(Self::UndelegateEphemeralAtaPermission),
            9 => Ok(Self::ResetEphemeralAtaPermission),
            10 => Ok(Self::CloseEphemeralAta),
            11 => Ok(Self::InitializeShuttleEphemeralAta),
            12 => Ok(Self::InitializeTransferQueue),
            13 => Ok(Self::DelegateShuttleEphemeralAta),
            14 => Ok(Self::UndelegateAndCloseShuttleToOwner),
            15 => Ok(Self::MergeShuttleIntoEphemeralAta),
            16 => Ok(Self::DepositAndQueueTransfer),
            17 => Ok(Self::EnsureTransferQueueCrank),
            19 => Ok(Self::DelegateTransferQueue),
            20 => Ok(Self::SponsoredLamportsTransfer),
            23 => Ok(Self::InitializeRentPda),
            24 => Ok(Self::SetupAndDelegateShuttleEphemeralAtaWithMerge),
            25 => Ok(Self::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransfer),
            26 => Ok(Self::WithdrawThroughDelegatedShuttleWithMerge),
            27 => Ok(Self::AllocateTransferQueue),
            28 => Ok(Self::ExecutePendingTransferQueueRefill),
            29 => Ok(Self::SchedulePrivateTransfer),
            30 => Ok(Self::ExecuteScheduledPrivateTransfer),
            31 => Ok(
                Self::DepositAndDelegateShuttleEphemeralAtaWithMergeAndPrivateTransferAndStashClose,
            ),
            _ => Err(()),
        }
    }
}
