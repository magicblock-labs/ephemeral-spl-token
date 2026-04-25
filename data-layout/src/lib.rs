use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemStruct};

mod fixed_layout;
mod variable_layout;

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
///     #[vardata(capacity = 72)]
///     encrypted_destination: Vec<u8>,
///     #[vardata(flexible = 1)]
///     encrypted_data_suffix: Vec<u8>,
/// }
///
/// #[fixed_offset_layout(assert_len = 253)]
/// struct DepositAndDelegateShuttleWithPrivateTransferArgs {
///     shuttle_id: u32,
///     amount: u64,
///     validator: Option<[u8; 32]>,
///     #[capacity = 72]
///     encrypted_destination: Vec<u8>,
///     #[flexible = 1]
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
///   - pub fn decode(bytes: &[u8]) -> Result<SelfView, ProgramError>
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

///
/// Usage
/// =====
///
/// ```ignore
///
/// #[variable_offset_layout(buffer_offset = 1)]
/// struct DepositAndDelegateShuttleWithPrivateTransferArgs {
///     shuttle_id: u32,
///     amount: u64,
///     validator: Option<[u8; 32]>,
///     #[flexible = 1]
///     encrypted_destination: Vec<u8>,
///     #[flexible = 2]
///     encrypted_data_suffix: Vec<u8>,
/// }
///
/// #[variable_offset_layout(buffer_offset = 1, option = implicit)]
/// struct DepositAndDelegateShuttleArgs {
///     shuttle_id: u32,
///     validator: Option<[u8; 32]>,
///     amount: u64,
/// }
///
/// ```
///
/// Attributes
/// ==========
///
/// Struct attributes:
///   - #[variable_offset_layout(buffer_offset = 0..7)]
///   - #[variable_offset_layout(buffer_offset = 0..7, option = implicit)]
///
///     - buffer_offset
///
///       Mandatory.
///
///       It specifies the offset of the input slice pointer from the previous
///       8-byte aligned base address, i.e:
///
///       `(bytes.as_ptr() as usize) % 8`
///
///       Example:
///
///       - if the original instruction input buffer is 8-byte aligned and
///         the payload slice passed to decode() is &input[1..], then
///         buffer_offset = 1.
///
///       The macro uses this contract both at runtime and at compile-time:
///
///       - decode() validates that the actual slice pointer matches
///         this offset
///       - borrowed getters are only generated when their alignment can be
///         guaranteed for every valid encoding under this buffer_offset
///
///     - option = implicit
///
///       Optional.
///
///       This enables compact Option<T> encoding without a tag byte. It is
///       supported only when the struct has no Vec fields and the payload
///       sizes of its Option<T> fields have unique subset sums.
///
///       In other words, every valid present/absent combination of the
///       implicit options must produce a distinct total encoded length.
///
///       Encoding:
///
///       - None omits the optional payload entirely
///       - Some(value) writes only the payload bytes
///
///       The generated decode() accepts only the total lengths implied by the
///       valid combinations of those implicit options.
///
/// Field attributes:
///   - #[flexible = 1|2]
///
///     - Mandatory: yes, field-type: Vec
///
///     - Examples
///
///       - #[flexible = 1]
///       - #[flexible = 2]
///
///     The number indicates the width, in bytes, used to encode Vec length.
///
///     - #[flexible = 1]
///
///       Vec length is encoded as u8, so len <= 255.
///
///     - #[flexible = 2]
///
///       Vec length is encoded as u16, so len <= 65535.
///
/// APIs
/// ====
///
/// Fields:
///   - pub const DATA_LEN: usize
///     when the layout has exactly one valid encoded length
///   - pub const DATA_LENS: [usize; N]
///     when the layout has no Vec fields and finitely many exact valid lengths
///   - pub const DATA_LEN_RANGE: (usize, usize)
///     when the layout contains a Vec field
///
/// Methods:
///   - pub fn decode(bytes: &[u8]) -> Result<SelfView, ProgramError>
///   - pub fn encode(&self) -> Result<Vec<u8>, ProgramError>
///   - pub fn encode_to(&self, bytes: &mut [u8]) -> Result<(), ProgramError>
///
#[proc_macro_attribute]
pub fn variable_offset_layout(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_string = attr.to_string();
    let input = parse_macro_input!(item as ItemStruct);

    match variable_layout::expand_variable_offset_layout(&attr_string, &input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
