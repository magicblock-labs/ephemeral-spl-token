use alloc::vec::Vec;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ESplInternalInstruction {
    /// 196 - DLP undelegatation callback used to restore delegated state.
    ///       The reason this discriminator is 196, not 200 or so, is because
    ///       its value is decided by the DLP program. So the discontinuity is intentional
    ///       and it is to indicate its origin.
    UndelegationCallback = 196,

    /// 201 - SettleAndCloseShuttleIntent: Magic standalone action that withdraws any
    ///       remaining shuttle balance to the supplied destination token account, then
    ///       closes the shuttle accounts.
    SettleAndCloseShuttleIntent = 201,

    /// 202 - ExecuteReadyQueuedTransfer: Magic standalone action that settles one queued transfer.
    ExecuteReadyQueuedTransfer = 202,

    /// 203 - ProcessTransferQueueTick: recurring crank callback that checks a queue and schedules settlement.
    ProcessTransferQueueTick = 203,

    /// 204 - TransferLamportsPda: post-delegation action that transfers the
    ///       requested lamports from the delegated zero-data PDA to the
    ///       destination base-layer account.
    TransferLamportsPda = 204,

    /// 205 - UndelegateLamportsPda: post-delegation action that commits and
    ///       undelegates the lamports PDA, then schedules the close/refund intent.
    UndelegateLamportsPda = 205,

    /// 206 - CloseLamportsPdaIntent: post-undelegate Magic intent that
    ///       refunds the sponsored rent to the global rent PDA and closes the PDA.
    CloseLamportsPdaIntent = 206,

    /// 207 - MarkTransferQueueRefillPending: Magic standalone action that
    ///       sets the per-queue refill-state pending flag.
    MarkTransferQueueRefillPending = 207,
}

impl ESplInternalInstruction {
    #[inline(always)]
    pub(crate) const fn discriminator(self) -> u8 {
        self as u8
    }

    #[inline(always)]
    pub(crate) const fn to_bytes(self) -> [u8; 1] {
        [self.discriminator()]
    }

    #[inline(always)]
    #[allow(dead_code)]
    pub(crate) fn to_vec(self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }

    #[inline(always)]
    pub(crate) fn with_data(self, instruction_data: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(1 + instruction_data.len());
        data.extend_from_slice(&self.to_bytes());
        data.extend_from_slice(instruction_data);
        data
    }
}

impl TryFrom<u8> for ESplInternalInstruction {
    type Error = ();

    #[inline(always)]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            196 => Ok(Self::UndelegationCallback),
            201 => Ok(Self::SettleAndCloseShuttleIntent),
            202 => Ok(Self::ExecuteReadyQueuedTransfer),
            203 => Ok(Self::ProcessTransferQueueTick),
            204 => Ok(Self::TransferLamportsPda),
            205 => Ok(Self::UndelegateLamportsPda),
            206 => Ok(Self::CloseLamportsPdaIntent),
            207 => Ok(Self::MarkTransferQueueRefillPending),
            _ => Err(()),
        }
    }
}
