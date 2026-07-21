use ephemeral_rollups_pinocchio::acl::{
    consts::PERMISSION_PROGRAM_ID,
    pda::permission_pda_from_permissioned_account,
    types::{Member, MemberFlags},
};
use ephemeral_spl_api::{
    require_eq_keys, require_ok,
    state::{
        group_receipt,
        group_receipt::GroupReceipt,
        transfer_queue::{queue_views_checked, QUEUE_SEED},
    },
};
use pinocchio::{
    cpi::{invoke_signed_with_bounds, Seed, Signer},
    error::ProgramError,
    instruction::{InstructionAccount, InstructionView},
    AccountView, ProgramResult,
};
use solana_address::Address;

use super::{
    ephemeral_account::{close_ephemeral_account, create_ephemeral_account},
    group_receipt::GROUP_RECEIPT_SEED,
};

/// ACL discriminator for `CreateEphemeralPermission` (not in ephemeral-rollups-pinocchio 0.10.x).
const CREATE_EPHEMERAL_PERMISSION_DISCRIMINATOR: u64 = 6;

/// ACL discriminator for `CloseEphemeralPermission` (not in ephemeral-rollups-pinocchio 0.10.x).
const CLOSE_EPHEMERAL_PERMISSION_DISCRIMINATOR: u64 = 8;

/// Accounts for the ephemeral ACL permission that keeps a `GroupReceipt` readable
/// only by its source while settlement signatures accumulate in it.
pub(crate) struct GroupReceiptPermissionAccounts<'a> {
    pub(crate) permission_info: &'a AccountView,
    pub(crate) permission_program: &'a AccountView,
}

/// Required accounts for control over receipt
pub(crate) struct GroupReceiptAccounts<'a> {
    pub(crate) group_receipt_info: &'a AccountView,
    pub(crate) queue_info: &'a AccountView,
    pub(crate) source: &'a AccountView,
    pub(crate) magic_vault: &'a AccountView,
    pub(crate) magic_program: &'a AccountView,
    /// Present on the extended account shapes; `None` preserves the legacy
    /// behavior of a receipt without an ACL permission.
    pub(crate) permission: Option<GroupReceiptPermissionAccounts<'a>>,
}

/// Creates `GroupReceipt` and initializes it.
/// Use this when the receipt account does not yet exist.
///
/// When permission accounts are provided, also creates a private ephemeral ACL
/// permission on the receipt (member: source) so the settlement signatures it
/// accumulates are not readable by third parties through the private RPC.
pub(crate) fn group_receipt_create<'a>(
    accounts: &GroupReceiptAccounts<'a>,
    group_receipt_bump: u8,
    group_id: u32,
    splits: u32,
) -> Result<GroupReceipt<'a>, ProgramError> {
    let (header, _) = queue_views_checked(unsafe { accounts.queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];
    let queue_signer = Signer::from(&queue_signer_seeds);

    let group_id_bytes = group_id.to_le_bytes();
    let receipt_bump_seed = [group_receipt_bump];
    let receipt_signer_seeds = [
        Seed::from(GROUP_RECEIPT_SEED),
        Seed::from(accounts.queue_info.address().as_ref()),
        Seed::from(accounts.source.address().as_ref()),
        Seed::from(group_id_bytes.as_ref()),
        Seed::from(&receipt_bump_seed),
    ];
    let receipt_signer = Signer::from(&receipt_signer_seeds);

    let space = GroupReceipt::required_size(splits as usize);

    require_ok!(create_ephemeral_account(
        accounts.queue_info,
        accounts.group_receipt_info,
        accounts.magic_vault,
        space.try_into().map_err(|_| ProgramError::ArithmeticOverflow)?,
        &[queue_signer, receipt_signer],
    ));

    require_ok!(group_receipt::initialize_group_receipt(
        accounts.group_receipt_info,
        group_id,
        splits,
        group_receipt_bump,
    ));

    if let Some(permission) = &accounts.permission {
        let queue_signer = Signer::from(&queue_signer_seeds);
        let receipt_signer = Signer::from(&receipt_signer_seeds);
        require_ok!(create_group_receipt_permission(
            accounts,
            permission,
            &[queue_signer, receipt_signer],
        ));
    }

    GroupReceipt::new(accounts.group_receipt_info)
}

/// Closes the group receipt account, refunding rent to the queue PDA.
/// Consumes the receipt since the account is no longer valid after closing.
///
/// When permission accounts are provided, first closes the receipt's ephemeral
/// ACL permission (rent refunds to the queue). Skipped for receipts created by
/// the legacy account shape, which have no permission.
pub(crate) fn group_receipt_close(
    accounts: &GroupReceiptAccounts<'_>,
    group_receipt: GroupReceipt<'_>,
) -> ProgramResult {
    let (header, _) = queue_views_checked(unsafe { accounts.queue_info.borrow_unchecked() })?;
    let queue_bump_seed = [header.bump];
    let queue_signer_seeds = [
        Seed::from(QUEUE_SEED),
        Seed::from(header.mint.as_ref()),
        Seed::from(header.validator.as_ref()),
        Seed::from(&queue_bump_seed),
    ];

    if let Some(permission) = &accounts.permission {
        let group_id_bytes = group_receipt.id().to_le_bytes();
        let receipt_bump_seed = [group_receipt.bump()];
        let receipt_signer_seeds = [
            Seed::from(GROUP_RECEIPT_SEED),
            Seed::from(accounts.queue_info.address().as_ref()),
            Seed::from(accounts.source.address().as_ref()),
            Seed::from(group_id_bytes.as_ref()),
            Seed::from(&receipt_bump_seed),
        ];
        let queue_signer = Signer::from(&queue_signer_seeds);
        let receipt_signer = Signer::from(&receipt_signer_seeds);
        require_ok!(close_group_receipt_permission(
            accounts,
            permission,
            &[queue_signer, receipt_signer],
        ));
    }

    let queue_signer = Signer::from(&queue_signer_seeds);
    close_ephemeral_account(
        accounts.queue_info,
        accounts.group_receipt_info,
        accounts.magic_vault,
        &[queue_signer],
    )
}

/// Creates a private ephemeral ACL permission on the group receipt, restricting
/// reads to the source. Rent is sponsored by the queue PDA (debited to the magic
/// vault, refunded on close).
///
/// Idempotent: a pre-existing permission is left untouched. This is safe because
/// the permission PDA derives from the receipt address, which bakes in the same
/// source; a stale permission from a missed close still grants only the source.
fn create_group_receipt_permission(
    accounts: &GroupReceiptAccounts<'_>,
    permission: &GroupReceiptPermissionAccounts<'_>,
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission.permission_program.address(),
        ProgramError::IncorrectProgramId
    );

    let expected_permission = permission_pda_from_permissioned_account(accounts.group_receipt_info.address());
    require_eq_keys!(
        &expected_permission,
        permission.permission_info.address(),
        ProgramError::InvalidSeeds
    );

    // Ephemeral accounts hold zero lamports on the ER; existence is data_len.
    if permission.permission_info.data_len() != 0 {
        return Ok(());
    }

    // [disc u64][is_private u8][member: flags u8 + pubkey 32]
    let mut data = [0u8; 42];
    data[..8].copy_from_slice(&CREATE_EPHEMERAL_PERMISSION_DISCRIMINATOR.to_le_bytes());
    data[8] = 1; // is_private
    let member = source_member(accounts.source.address());
    data[9] = member.flags.as_u8();
    data[10..42].copy_from_slice(member.pubkey.as_ref());

    invoke_signed_with_bounds::<5>(
        &InstructionView {
            program_id: &PERMISSION_PROGRAM_ID,
            accounts: &[
                InstructionAccount::writable_signer(accounts.queue_info.address()),
                InstructionAccount::readonly_signer(accounts.group_receipt_info.address()),
                InstructionAccount::writable(permission.permission_info.address()),
                InstructionAccount::writable(accounts.magic_vault.address()),
                InstructionAccount::readonly(accounts.magic_program.address()),
            ],
            data: &data,
        },
        &[
            accounts.queue_info,
            accounts.group_receipt_info,
            permission.permission_info,
            accounts.magic_vault,
            accounts.magic_program,
        ],
        signers,
    )
}

/// Closes the group receipt's ephemeral ACL permission, refunding rent to the
/// queue PDA. The receipt PDA authorizes the close by signing as the
/// permissioned account. No-op when the permission does not exist (receipts
/// created by the legacy account shape).
fn close_group_receipt_permission(
    accounts: &GroupReceiptAccounts<'_>,
    permission: &GroupReceiptPermissionAccounts<'_>,
    signers: &[Signer<'_, '_>],
) -> ProgramResult {
    require_eq_keys!(
        &PERMISSION_PROGRAM_ID,
        permission.permission_program.address(),
        ProgramError::IncorrectProgramId
    );

    // Ephemeral accounts hold zero lamports on the ER; existence is data_len.
    if permission.permission_info.data_len() == 0 {
        return Ok(());
    }

    invoke_signed_with_bounds::<6>(
        &InstructionView {
            program_id: &PERMISSION_PROGRAM_ID,
            accounts: &[
                InstructionAccount::writable_signer(accounts.queue_info.address()),
                InstructionAccount::readonly(accounts.queue_info.address()),
                InstructionAccount::readonly_signer(accounts.group_receipt_info.address()),
                InstructionAccount::writable(permission.permission_info.address()),
                InstructionAccount::writable(accounts.magic_vault.address()),
                InstructionAccount::readonly(accounts.magic_program.address()),
            ],
            data: &CLOSE_EPHEMERAL_PERMISSION_DISCRIMINATOR.to_le_bytes(),
        },
        &[
            accounts.queue_info,
            accounts.queue_info,
            accounts.group_receipt_info,
            permission.permission_info,
            accounts.magic_vault,
            accounts.magic_program,
        ],
        signers,
    )
}

/// Read-only membership for the source: transaction logs, balances, message,
/// and account signatures — but not `AUTHORITY`, so the permission lifecycle
/// stays program-managed for the receipt's whole lifetime.
fn source_member(source: &Address) -> Member {
    let mut flags = MemberFlags::new();
    flags.set(MemberFlags::TX_LOGS);
    flags.set(MemberFlags::TX_BALANCES);
    flags.set(MemberFlags::TX_MESSAGE);
    flags.set(MemberFlags::ACCOUNT_SIGNATURES);
    Member { flags, pubkey: *source }
}

#[cfg(feature = "logging")]
#[inline(never)]
pub(crate) fn group_receipt_log(group_receipt: &GroupReceipt<'_>) {
    use alloc::string::ToString;

    pinocchio_log::log!(
        "All transfers complete for group id: {} splits: {}",
        group_receipt.id(),
        group_receipt.splits()
    );
    if let Ok(items) = group_receipt.items() {
        for (i, item) in items.iter().enumerate() {
            match item.signature() {
                Some(sig) => pinocchio_log::log!(
                    "transfer[{}], ok: {}, amount: {}, sig: {}",
                    i as u32,
                    item.ok(),
                    item.amount(),
                    sig.to_string().as_str()
                ),
                None => pinocchio_log::log!(
                    "transfer[{}], ok: {}, amount: {}, sig: None",
                    i as u32,
                    item.ok(),
                    item.amount()
                ),
            }
        }
    }
}
