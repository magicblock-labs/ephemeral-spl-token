use data_layout::variable_offset_layout;

#[repr(align(8))]
struct Aligned<const N: usize>([u8; N]);

#[variable_offset_layout]
struct PrivateTransferArgs {
    shuttle_id: u32,
    amount: u64,
    validator: Option<[u8; 32]>,
    #[flexible = 1]
    encrypted_destination: Vec<u8>,
    #[flexible = 2]
    encrypted_data_suffix: Vec<u8>,
}

#[test]
fn variable_layout_private_args() {
    assert_eq!(PrivateTransferArgs::MIN_DATA_LEN, 12);
    assert_eq!(
        PrivateTransferArgs::MAX_DATA_LEN,
        12 + (1 + 32) + (1 + 0xFF) + (2 + 0xFFFF)
    );
}
