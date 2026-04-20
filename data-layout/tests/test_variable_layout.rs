use data_layout::variable_offset_layout;
use pinocchio::error::ProgramError;

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
    assert_eq!(PrivateTransferArgs::MIN_DATA_LEN, 16);
    assert_eq!(
        PrivateTransferArgs::MAX_DATA_LEN,
        12 + (1 + 32) + (1 + 0xFF) + (2 + 0xFFFF)
    );

    let value = PrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![1, 2, 3, 4],
        encrypted_data_suffix: vec![10, 20, 30, 40, 50, 60, 70, 80],
    };

    let mut aligned =
        Aligned([0; PrivateTransferArgs::MIN_DATA_LEN + (1 + 32) + (1 + 4) + (2 + 8)]);

    assert!(aligned.0.len() <= PrivateTransferArgs::MAX_DATA_LEN);

    let bytes = &mut aligned.0;

    // shuttle_id: u32 (offset: 0)
    bytes[0..4].copy_from_slice(&100_u32.to_le_bytes());

    // amount: u64 (offset: 4)
    bytes[4..12].copy_from_slice(&200_u64.to_le_bytes());

    // validator: Option<[u8; 32]> (offset: 12)
    bytes[12] = 1;
    bytes[13..45].copy_from_slice(&[1; 32]);

    // encrypted_destination: Vec<u8> (offset: 45, len_width = 1)
    bytes[45] = 4;
    bytes[46..50].copy_from_slice(&[1, 2, 3, 4]);

    // encrypted_data_suffix: Vec<u8> (offset: 50, len_width = 2)
    bytes[50..52].copy_from_slice(&8_u16.to_le_bytes());
    bytes[52..60].copy_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);

    let view = PrivateTransferArgs::try_view_from(bytes).unwrap();

    assert_eq!(view.shuttle_id(), 100);
    assert_eq!(view.amount(), 200);
    assert_eq!(view.validator(), Some(&[1; 32]));
    assert_eq!(view.encrypted_destination(), &[1, 2, 3, 4]);
    assert_eq!(
        view.encrypted_data_suffix(),
        &[10, 20, 30, 40, 50, 60, 70, 80]
    );

    // let encoded = value.encode();
    // assert_eq!(encoded, Ok(aligned.0.into()));
    // let encoded = encoded.unwrap();

    // let mut encoded_out = vec![255; aligned.0.len() + 4];
    // value.encode_to(&mut encoded_out).unwrap();

    // assert_eq!(&encoded_out[..aligned.0.len()], &encoded);

    // // the last 4 bytes must not be overwritten by encode_to()
    // assert_eq!(&encoded_out[aligned.0.len()..], &[255, 255, 255, 255]);
}

#[variable_offset_layout]
struct VariableOffsetViewArgs {
    header: u16,
    validator: Option<u32>,
    #[flexible = 1]
    payload: Vec<u8>,
    amount: u64,
    checksum: u16,
}

#[test]
fn variable_layout_computes_offsets_after_variable_fields() {
    assert_eq!(VariableOffsetViewArgs::MIN_DATA_LEN, 14);
    assert_eq!(
        VariableOffsetViewArgs::MAX_DATA_LEN,
        2 + (1 + 4) + (1 + 0xFF) + 8 + 2
    );

    let mut aligned = Aligned([0; 21]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&7_u16.to_le_bytes());
    bytes[2] = 1;
    bytes[3..7].copy_from_slice(&9_u32.to_le_bytes());
    bytes[7] = 3;
    bytes[8..11].copy_from_slice(&[1, 2, 3]);
    bytes[11..19].copy_from_slice(&77_u64.to_le_bytes());
    bytes[19..21].copy_from_slice(&0xBEEF_u16.to_le_bytes());

    let view = VariableOffsetViewArgs::try_view_from(bytes).unwrap();

    assert_eq!(view.header(), 7);
    assert_eq!(view.validator(), Some(9));
    assert_eq!(view.payload(), &[1, 2, 3]);
    assert_eq!(view.amount(), 77);
    assert_eq!(view.checksum(), 0xBEEF);
}

#[test]
fn variable_layout_handles_none_and_empty_vec_before_trailing_fields() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::MIN_DATA_LEN]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&5_u16.to_le_bytes());
    bytes[2] = 0;
    bytes[3] = 0;
    bytes[4..12].copy_from_slice(&55_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&9_u16.to_le_bytes());

    let view = VariableOffsetViewArgs::try_view_from(bytes).unwrap();

    assert_eq!(view.header(), 5);
    assert_eq!(view.validator(), None);
    assert_eq!(view.payload(), &[]);
    assert_eq!(view.amount(), 55);
    assert_eq!(view.checksum(), 9);
}

#[test]
fn variable_layout_try_view_from_rejects_invalid_option_tag() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::MIN_DATA_LEN]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&1_u16.to_le_bytes());
    bytes[2] = 2;
    bytes[3] = 0;
    bytes[4..12].copy_from_slice(&11_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&13_u16.to_le_bytes());

    assert_eq!(
        VariableOffsetViewArgs::try_view_from(bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[test]
fn variable_layout_try_view_from_rejects_truncated_vec_payload() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::MIN_DATA_LEN]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&1_u16.to_le_bytes());
    bytes[2] = 0;
    bytes[3] = 11;
    bytes[4..12].copy_from_slice(&11_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&13_u16.to_le_bytes());

    assert_eq!(
        VariableOffsetViewArgs::try_view_from(bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[variable_offset_layout]
struct BorrowedAfterVariableArgs {
    tag: u8,
    #[flexible = 1]
    prefix: Vec<u8>,
    values: [u64; 2],
}

#[test]
fn variable_layout_allows_aligned_borrowed_fields_after_variable_data() {
    let mut aligned = Aligned([0; 24]);
    let bytes = &mut aligned.0;

    bytes[0] = 1;
    bytes[1] = 6;
    bytes[2..8].copy_from_slice(&[9, 9, 9, 9, 9, 9]);
    bytes[8..24].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);

    let view = BorrowedAfterVariableArgs::try_view_from(bytes).unwrap();

    assert_eq!(view.tag(), 1);
    assert_eq!(view.prefix(), &[9, 9, 9, 9, 9, 9]);
    let _: &[u64; 2] = view.values();
    assert_eq!(view.values(), &[1, 2]);
}

#[test]
fn variable_layout_rejects_misaligned_borrowed_fields_after_variable_data() {
    let mut aligned = Aligned([0; 19]);
    let bytes = &mut aligned.0;

    bytes[0] = 1;
    bytes[1] = 1;
    bytes[2] = 9;
    bytes[3..19].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);

    assert_eq!(
        BorrowedAfterVariableArgs::try_view_from(bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}
