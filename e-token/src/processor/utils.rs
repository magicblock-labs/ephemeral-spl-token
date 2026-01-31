use core::slice::from_raw_parts;
use pinocchio::{
    cpi::invoke_signed, cpi::Signer, instruction::InstructionAccount, instruction::InstructionView,
    AccountView, ProgramResult,
};

const UNINIT_BYTE: u8 = 0u8;

/// Write bytes to a mutable slice
#[inline]
fn write_bytes(dst: &mut [u8], src: &[u8]) {
    dst.copy_from_slice(src);
}

/// Transfer Tokens from one Token Account to another.
///
/// ### Accounts:
///   0. `[WRITE]` The source account.
///   1. `[]` The token mint.
///   2. `[WRITE]` The destination account.
///   3. `[SIGNER]` The source account's owner/delegate.
///   4. `[]` The token program.
pub struct TransferChecked<'a> {
    /// Sender account.
    pub from: &'a AccountView,
    /// Mint Account
    pub mint: &'a AccountView,
    /// Recipient account.
    pub to: &'a AccountView,
    /// Authority account.
    pub authority: &'a AccountView,
    /// Token program account.
    pub token_program: &'a AccountView,
    /// Amount of micro-tokens to transfer.
    pub amount: u64,
    /// Decimal for the Token
    pub decimals: u8,
}

impl TransferChecked<'_> {
    #[inline(always)]
    pub fn invoke(&self) -> ProgramResult {
        self.invoke_signed(&[])
    }

    #[inline(always)]
    pub fn invoke_signed(&self, signers: &[Signer]) -> ProgramResult {
        // Instruction accounts
        let instruction_accounts: [InstructionAccount; 4] = [
            InstructionAccount::writable(self.from.address()),
            InstructionAccount::readonly(self.mint.address()),
            InstructionAccount::writable(self.to.address()),
            InstructionAccount::readonly_signer(self.authority.address()),
        ];

        // Instruction data layout:
        // -  [0]: instruction discriminator (1 byte, u8)
        // -  [1..9]: amount (8 bytes, u64)
        // -  [9]: decimals (1 byte, u8)
        let mut instruction_data = [UNINIT_BYTE; 10];

        // Set discriminator as u8 at offset [0]
        write_bytes(&mut instruction_data, &[12]);
        // Set amount as u64 at offset [1..9]
        write_bytes(&mut instruction_data[1..9], &self.amount.to_le_bytes());
        // Set decimals as u8 at offset [9]
        write_bytes(&mut instruction_data[9..], &[self.decimals]);

        // Invoke the token program with the custom token_program address
        let instruction = InstructionView {
            program_id: self.token_program.address(),
            accounts: &instruction_accounts,
            data: unsafe { from_raw_parts(instruction_data.as_ptr() as _, 10) },
        };

        invoke_signed(
            &instruction,
            &[self.from, self.mint, self.to, self.authority],
            signers,
        )
    }
}
