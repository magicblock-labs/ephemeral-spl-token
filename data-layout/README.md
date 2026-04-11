# data-layout

Minimal zero-copy layout-view proc macro.

Encoding rules:

- Fixed-size types are encoded with bytemuck. No extra bytes are needed to encode their size.
- Variable-size types such as `String` must have `#[max_len = N]`.
  - `max_len <= 0xFF` uses 1 byte for the inner length
  - `max_len <= 0xFFFF` uses 2 bytes
  - `max_len <= 0xFFFF_FFFF` uses 4 bytes
- `Option<T>` is encoded as `1 + space(T)` bytes:
  - first byte is a presence tag
  - `0 => None`
  - `1 => Some(T)`
  - any other tag is logged with the field name and returns `InvalidInstructionData`

Validation rules:

- validation happens aggressively during view construction
- getters do not return `Result<>`
- after `try_view_from(...)` succeeds, getters may rely on the layout being valid

# Usage

```rust
use data_layout::variable_layout;

#[variable_layout]
pub struct DepositAndDelegateShuttleArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<[u8; 32]>,
    #[max_len = 8]
    pub destination: Option<String>,
}
```

Mutable view generation is opt-in:

```rust
#[variable_layout(mut)]
pub struct DepositAndDelegateShuttleArgs {
    pub shuttle_id: u32,
    pub amount: u64,
    pub validator: Option<[u8; 32]>,
    #[max_len = 8]
    pub destination: Option<String>,
}
```

## Supported fields

- Required fields:
  - integer primitives
  - fixed-size arrays of integer primitives
  - `String` with `#[max_len = N]`
- Optional fields:
  - `Option<T>` where `T` is any supported required-field type

## Wire format

The total layout size is fixed.

Examples:

- `u64` occupies `8`
- `[u8; 32]` occupies `32`
- `String` with `#[max_len = 8]` occupies `1 + 8`
- `Option<[u8; 32]>` occupies `1 + 32`
- `Option<String>` with `#[max_len = 8]` occupies `1 + 1 + 8`

So offsets are static:

```text
[fixed field][optional tag][optional reserved payload][next field]...
```

## Generated code

The macro keeps the original struct and generates:

```rust
pub struct DepositAndDelegateShuttleArgsView<'a> { ... }
pub struct DepositAndDelegateShuttleArgsViewMut<'a> { ... } // only with #[variable_layout(mut)]

impl DepositAndDelegateShuttleArgs {
    pub const DATA_LEN: usize = ...;

    pub fn try_view_from(bytes: &[u8]) -> Result<DepositAndDelegateShuttleArgsView<'_>, ProgramError>;
    pub fn try_view_from_mut(bytes: &mut [u8]) -> Result<DepositAndDelegateShuttleArgsViewMut<'_>, ProgramError>;
}
```

## Mutation rules

- mutable views do not change layout shape
- `None -> Some(...)` is rejected
- variable-size fields can only be updated with the same byte length as the existing value
- optional fields can only be mutated when already present
