use crate::processor::internal::callbacks::MagicResponseView;
use crate::processor::internal::refund::{
    schedule_refund_on_failure, RefundOnFailureAccounts, RefundOnFailureArgs,
};
use ephemeral_spl_api::require_n_accounts;
use pinocchio::{AccountView, ProgramResult};

///
/// Executes on: ER only.
///
/// Accounts:
///
///  0: [signer]   - PDA     : Callback signer (CALLBACK_SIGNER).
///  1: []         - Any     : Refund destination owner.
///  2: [writable] - PDA     : Transfer queue account.
///  3: [writable] - PDA     : Magic fee vault account.
///  4: [writable] - PDA     : Magic context account.
///  5: []         - Program : Magic program.
///
/// Instruction Data: MagicResponse
///
#[inline(never)]
pub fn process_refund_on_failure_callback(
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    let [callback_signer, refund_destination_owner, queue_info, magic_fee_vault_info, magic_context_info, magic_program_info] =
        require_n_accounts!(accounts, 6);
    let accounts = RefundOnFailureAccounts::try_new(
        callback_signer,
        refund_destination_owner,
        queue_info,
        magic_fee_vault_info,
        magic_context_info,
        magic_program_info,
    )?;

    let response = MagicResponseView::deserialize(instruction_data)?;
    let args = RefundOnFailureArgs::decode(response.data)?;

    if response.ok {
        #[cfg(feature = "logging")]
        {
            use alloc::string::ToString;
            if let Some(signature) = response.signature {
                pinocchio_log::log!(
                    "Amount refunded: {}, signature: {}",
                    args.amount(),
                    signature.to_string().as_str()
                );
            }
        }

        return Ok(());
    }

    #[cfg(feature = "logging")]
    {
        use alloc::string::ToString;
        if let Some(signature) = response.signature {
            pinocchio_log::log!(
                "Failed to refund: {}, signature: {}",
                args.amount(),
                signature.to_string().as_str()
            );
        } else {
            pinocchio_log::log!("Failed to refund: {}", args.amount());
        }
    }

    schedule_refund_on_failure(&accounts, args.amount())
}
