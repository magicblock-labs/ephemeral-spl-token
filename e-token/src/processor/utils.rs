use core::slice::from_raw_parts;
use pinocchio::{
    cpi::invoke_signed, cpi::Signer, instruction::InstructionAccount, instruction::InstructionView,
    Address, ProgramResult,
};
use pinocchio_token::state::AccountState;
use solana_account_view::{AccountView, Ref};
use solana_program_error::ProgramError;

const UNINIT_BYTE: u8 = 0u8;

/// SPL Token Program
/// Address: TokenkegQfeZyiNwAJsyFbPVwwQQfubbs5nWvs15on
const SPL_TOKEN_ADDRESS: Address = Address::new_from_array([
    6, 221, 246, 225, 215, 101, 161, 147, 217, 76, 129, 243, 40, 72, 73, 156, 147, 147, 164, 80,
    144, 122, 30, 235, 248, 137, 231, 100, 59, 70, 112, 65,
]);

/// SPL Token Program 2022
/// Address: TokenzQdBbjWhAr21LWp34Vnues9Cerabl5MZdsV
const SPL_TOKEN_2022_ADDRESS: Address = Address::new_from_array([
    38, 8, 80, 80, 188, 112, 76, 32, 253, 1, 3, 132, 65, 206, 171, 35, 183, 239, 156, 174, 233,
    101, 220, 198, 44, 227, 66, 249, 202, 30, 112, 77,
]);

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

/// Mint data.
#[repr(C)]
#[allow(dead_code)]
pub struct Mint {
    /// Indicates whether the mint authority is present or not.
    mint_authority_flag: [u8; 4],

    /// Optional authority used to mint new tokens. The mint authority may only
    /// be provided during mint creation. If no mint authority is present
    /// then the mint has a fixed supply and no further tokens may be
    /// minted.
    mint_authority: Address,

    /// Total supply of tokens.
    supply: [u8; 8],

    /// Number of base 10 digits to the right of the decimal place.
    decimals: u8,

    /// Is `true` if this structure has been initialized.
    is_initialized: u8,

    /// Indicates whether the freeze authority is present or not.
    freeze_authority_flag: [u8; 4],

    /// Optional authority to freeze token accounts.
    freeze_authority: Address,
}

#[allow(dead_code)]
impl Mint {
    /// The length of the `Mint` account data.
    pub const BASE_LEN: usize = core::mem::size_of::<Mint>();

    /// Return a `Mint` from the given account view.
    ///
    /// This method performs owner and length validation on `AccountView`, safe borrowing
    /// the account data.
    #[inline]
    pub fn from_account_view(account_view: &AccountView) -> Result<Ref<Mint>, ProgramError> {
        if account_view.data_len() < Self::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account_view.owned_by(&SPL_TOKEN_ADDRESS)
            && !account_view.owned_by(&SPL_TOKEN_2022_ADDRESS)
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Ref::map(account_view.try_borrow()?, |data| unsafe {
            Self::from_bytes_unchecked(data)
        }))
    }

    /// Return a `Mint` from the given account view.
    ///
    /// This method performs owner and length validation on `AccountView`, but does not
    /// perform the borrow check.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it is safe to borrow the account data (e.g., there are
    /// no mutable borrows of the account data).
    #[inline]
    pub unsafe fn from_account_view_unchecked(
        account_view: &AccountView,
    ) -> Result<&Self, ProgramError> {
        if account_view.data_len() < Self::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account_view.owned_by(&SPL_TOKEN_ADDRESS)
            && !account_view.owned_by(&SPL_TOKEN_2022_ADDRESS)
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Self::from_bytes_unchecked(account_view.borrow_unchecked()))
    }

    /// Return a `Mint` from the given bytes.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bytes` contains a valid representation of `Mint`, and
    /// it is properly aligned to be interpreted as an instance of `Mint`.
    /// At the moment `Mint` has an alignment of 1 byte.
    /// This method does not perform a length validation.
    #[inline(always)]
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        &*(bytes[..Self::BASE_LEN].as_ptr() as *const Mint)
    }

    #[inline(always)]
    pub fn has_mint_authority(&self) -> bool {
        self.mint_authority_flag[0] == 1
    }

    pub fn mint_authority(&self) -> Option<&Address> {
        if self.has_mint_authority() {
            Some(self.mint_authority_unchecked())
        } else {
            None
        }
    }

    /// Return the mint authority.
    ///
    /// This method should be used when the caller knows that the mint will have a mint
    /// authority set since it skips the `Option` check.
    #[inline(always)]
    pub fn mint_authority_unchecked(&self) -> &Address {
        &self.mint_authority
    }

    pub fn supply(&self) -> u64 {
        u64::from_le_bytes(self.supply)
    }

    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    pub fn is_initialized(&self) -> bool {
        self.is_initialized == 1
    }

    #[inline(always)]
    pub fn has_freeze_authority(&self) -> bool {
        self.freeze_authority_flag[0] == 1
    }

    pub fn freeze_authority(&self) -> Option<&Address> {
        if self.has_freeze_authority() {
            Some(self.freeze_authority_unchecked())
        } else {
            None
        }
    }

    /// Return the freeze authority.
    ///
    /// This method should be used when the caller knows that the mint will have
    /// a freeze authority set since it skips the `Option` check.
    #[inline(always)]
    pub fn freeze_authority_unchecked(&self) -> &Address {
        &self.freeze_authority
    }
}

/// Token account data.
#[repr(C)]
#[allow(dead_code)]
pub struct TokenAccount {
    /// The mint associated with this account
    mint: Address,

    /// The owner of this account.
    owner: Address,

    /// The amount of tokens this account holds.
    amount: [u8; 8],

    /// Indicates whether the delegate is present or not.
    delegate_flag: [u8; 4],

    /// If `delegate` is `Some` then `delegated_amount` represents
    /// the amount authorized by the delegate.
    delegate: Address,

    /// The account's state.
    state: u8,

    /// Indicates whether this account represents a native token or not.
    is_native: [u8; 4],

    /// When `is_native.is_some()` is `true`, this is a native token, and the
    /// value logs the rent-exempt reserve. An Account is required to be rent-exempt,
    /// so the value is used by the Processor to ensure that wrapped SOL
    /// accounts do not drop below this threshold.
    native_amount: [u8; 8],

    /// The amount delegated.
    delegated_amount: [u8; 8],

    /// Indicates whether the close authority is present or not.
    close_authority_flag: [u8; 4],

    /// Optional authority to close the account.
    close_authority: Address,
}

#[allow(dead_code)]
impl TokenAccount {
    pub const BASE_LEN: usize = core::mem::size_of::<TokenAccount>();

    /// Return a `TokenAccount` from the given account view.
    ///
    /// This method performs owner and length validation on `AccountView`, safe borrowing
    /// the account data.
    #[inline]
    pub fn from_account_view(
        account_view: &AccountView,
    ) -> Result<Ref<TokenAccount>, ProgramError> {
        if account_view.data_len() < Self::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account_view.owned_by(&SPL_TOKEN_ADDRESS)
            && !account_view.owned_by(&SPL_TOKEN_2022_ADDRESS)
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Ref::map(account_view.try_borrow()?, |data| unsafe {
            Self::from_bytes_unchecked(data)
        }))
    }

    /// Return a `TokenAccount` from the given account view.
    ///
    /// This method performs owner and length validation on `AccountView`, but does not
    /// perform the borrow check.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it is safe to borrow the account data (e.g., there are
    /// no mutable borrows of the account data).
    #[inline]
    pub unsafe fn from_account_view_unchecked(
        account_view: &AccountView,
    ) -> Result<&TokenAccount, ProgramError> {
        if account_view.data_len() < Self::BASE_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account_view.owned_by(&SPL_TOKEN_ADDRESS)
            && !account_view.owned_by(&SPL_TOKEN_2022_ADDRESS)
        {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Self::from_bytes_unchecked(account_view.borrow_unchecked()))
    }

    /// Return a `TokenAccount` from the given bytes.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `bytes` contains a valid representation of `TokenAccount`, and
    /// it is properly aligned to be interpreted as an instance of `TokenAccount`.
    /// At the moment `TokenAccount` has an alignment of 1 byte.
    /// This method does not perform a length validation.
    #[inline(always)]
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        &*(bytes.as_ptr() as *const TokenAccount)
    }

    pub fn mint(&self) -> &Address {
        &self.mint
    }

    pub fn owner(&self) -> &Address {
        &self.owner
    }

    pub fn amount(&self) -> u64 {
        u64::from_le_bytes(self.amount)
    }

    #[inline(always)]
    pub fn has_delegate(&self) -> bool {
        self.delegate_flag[0] == 1
    }

    pub fn delegate(&self) -> Option<&Address> {
        if self.has_delegate() {
            Some(self.delegate_unchecked())
        } else {
            None
        }
    }

    /// Use this when you know the account will have a delegate and want to skip the `Option` check.
    #[inline(always)]
    pub fn delegate_unchecked(&self) -> &Address {
        &self.delegate
    }

    #[inline(always)]
    pub fn state(&self) -> AccountState {
        self.state.into()
    }

    #[inline(always)]
    pub fn is_native(&self) -> bool {
        self.is_native[0] == 1
    }

    pub fn native_amount(&self) -> Option<u64> {
        if self.is_native() {
            Some(self.native_amount_unchecked())
        } else {
            None
        }
    }

    /// Return the native amount.
    ///
    /// This method should be used when the caller knows that the token is native since it
    /// skips the `Option` check.
    #[inline(always)]
    pub fn native_amount_unchecked(&self) -> u64 {
        u64::from_le_bytes(self.native_amount)
    }

    pub fn delegated_amount(&self) -> u64 {
        u64::from_le_bytes(self.delegated_amount)
    }

    #[inline(always)]
    pub fn has_close_authority(&self) -> bool {
        self.close_authority_flag[0] == 1
    }

    pub fn close_authority(&self) -> Option<&Address> {
        if self.has_close_authority() {
            Some(self.close_authority_unchecked())
        } else {
            None
        }
    }

    /// Return the close authority.
    ///
    /// This method should be used when the caller knows that the token will have a close
    /// authority set since it skips the `Option` check.
    #[inline(always)]
    pub fn close_authority_unchecked(&self) -> &Address {
        &self.close_authority
    }

    #[inline(always)]
    pub fn is_initialized(&self) -> bool {
        self.state != AccountState::Uninitialized as u8
    }

    #[inline(always)]
    pub fn is_frozen(&self) -> bool {
        self.state == AccountState::Frozen as u8
    }
}
