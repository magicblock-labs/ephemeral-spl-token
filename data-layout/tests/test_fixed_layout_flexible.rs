use data_layout::fixed_offset_layout;

#[repr(align(8))]
struct Aligned<const N: usize>([u8; N]);

#[fixed_offset_layout]
struct PrivateTransferArgs {
    shuttle_id: u32,
    amount: u64,
    validator: Option<[u8; 32]>,
    #[capacity = 80]
    encrypted_destination: Vec<u8>,
    #[flexible = 2]
    encrypted_data_suffix: Vec<u8>,
}

#[test]
fn fixed_layout_flexible_private_args() {
    assert_eq!(PrivateTransferArgs::MIN_DATA_LEN, 128);

    let value = PrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![1, 2, 3, 4],
        encrypted_data_suffix: vec![10, 20, 30, 40, 50, 60, 70, 80],
    };

    let mut aligned = Aligned([0; PrivateTransferArgs::MIN_DATA_LEN + 8]);

    assert!(aligned.0.len() <= PrivateTransferArgs::MAX_DATA_LEN);

    let bytes = &mut aligned.0;

    // shuttle_id: u32 (offset: 0)
    bytes[0..4].copy_from_slice(&100_u32.to_le_bytes());

    // amount: u64 (offset: 4)
    bytes[4..12].copy_from_slice(&200_u64.to_le_bytes());

    // validator: Option<[u8; 32]> (offset: 12)
    bytes[12] = 1;
    bytes[13..45].copy_from_slice(&[1; 32]);

    // encrypted_destination: Vec<u8> (offset: 45, len_width = 1, capacity = 80)
    bytes[45] = 4;
    bytes[46..50].copy_from_slice(&[1, 2, 3, 4]);

    // encrypted_data_suffix: Vec<8> (offset: 125, len_width = 2, capacity = 120)
    bytes[126..128].copy_from_slice(&8_u16.to_le_bytes());
    bytes[128..136].copy_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);

    let view = PrivateTransferArgs::try_view_from(bytes).unwrap();

    assert_eq!(view.shuttle_id(), 100);
    assert_eq!(view.amount(), 200);
    assert_eq!(view.validator(), Some(&[1; 32]));
    assert_eq!(view.encrypted_destination(), &[1, 2, 3, 4]);
    assert_eq!(
        view.encrypted_data_suffix(),
        &[10, 20, 30, 40, 50, 60, 70, 80]
    );

    assert_eq!(value.encode(), Ok(aligned.0.into()));
}

#[test]
fn fixed_layout_flexible_private_args_optional_vector() {
    assert_eq!(PrivateTransferArgs::MIN_DATA_LEN, 128);

    let value = PrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![1, 2, 3, 4],
        encrypted_data_suffix: vec![],
    };

    assert_eq!(
        value.encode().unwrap().len(),
        PrivateTransferArgs::MIN_DATA_LEN
    );
}

#[fixed_offset_layout]
struct FlexibleOptional {
    shuttle_id: u32,
    amount: u64,
    //    #[optional]
    validator: Option<[u8; 32]>,
}
