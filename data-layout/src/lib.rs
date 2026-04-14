use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemStruct};

mod fixed_layout;

///
/// Usage
/// =====
///
/// ```ignore
///
/// #[fixed_offset_layout(assert_len = 253)]
/// struct DepositAndDelegateShuttleWithPrivateTransferArgs {
///     shuttle_id: u32,
///     amount: u64,
///     validator: Option<[u8; 32]>,
///     #[capacity = 72]
///     encrypted_destination: Vec<u8>,
///     #[capacity = flexible]
///     encrypted_data_suffix: Vec<u8>,
/// }
///
/// ```
///
/// Attributes
/// ==========
///
/// Struct attributes:
///   - #[fixed_layout(assert_len = <compile-time-const>)]
///
///     assert_len is an optional attribute that developers can use
///     to detect size-change across versions of data-layout crate.
///
///     Note it cannot be applied on a struct that contains a Vec field
///     with #[capacity = flexible].
///
/// Field attributes:
///   - #[capacity = <compile-time-const> | flexible]
///
///     - Mandatory: yes, field-type: Vec
///
///     - Examples
///
///       - #[capacity = 120]
///       - #[capacity = flexible]
///
///     - <compile-time-const>
///
///       Applicable on Vec field only. It indicates the max number of elements
///       in the vector i.e the capacity of the vector and must be known at
///       compile-time. The encode() method will fail to encode a vector that
///       contains more elements than this capacity and ProgramError::InvalidRealloc
///       will be returned.
///
///       The capacity determines the number of bytes used to encode the length of
///       the vector, which can be either 1 byte or 2 bytes, which implies that
///       capacity <= 65535.
///
///     - flexible
///
///       Applicable on Vec field which also has to be the last field of the struct.
///       This value implies that the vector is allowed to have any length and there
///       is no capacity enforced. In this case, 2 bytes is used to encode the length
///       of the vector which indirectly implies that length <= 65535.
///
/// APIs
/// ====
///
/// Fields:
///   - pub const DATA_LEN: usize
///   - pub const OFFSETS: [usize; N]
///
/// Methods:
///   - pub fn try_view_from(bytes: &[u8]) -> Result<SelfView, ProgramError>
///   - pub fn encode(&self) -> Result<[u8; DATA_LEN], ProgramError>
///   - pub fn encode_to(&self, bytes: &mut [u8; DATA_LEN]) -> Result<(), ProgramError>
///
#[proc_macro_attribute]
pub fn fixed_offset_layout(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_string = attr.to_string();
    let input = parse_macro_input!(item as ItemStruct);

    match fixed_layout::expand_fixed_offset_layout(&attr_string, &input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
