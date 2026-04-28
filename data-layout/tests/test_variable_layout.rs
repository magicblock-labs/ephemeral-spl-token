use data_layout::variable_offset_layout;
use pinocchio::error::ProgramError;

#[repr(align(8))]
struct Aligned<const N: usize>([u8; N]);

#[variable_offset_layout(buffer_offset = 0)]
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
    assert_eq!(
        PrivateTransferArgs::DATA_LEN_RANGE,
        (16, 12 + (1 + 32) + (1 + 0xFF) + (2 + 0xFFFF))
    );

    let value = PrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![1, 2, 3, 4],
        encrypted_data_suffix: vec![10, 20, 30, 40, 50, 60, 70, 80],
    };

    let expected_len = 4 + 8 + (1 + 32) + (1 + 4) + (2 + 8);
    let mut aligned = Aligned([0; 4 + 8 + (1 + 32) + (1 + 4) + (2 + 8)]);

    assert!(aligned.0.len() <= PrivateTransferArgs::DATA_LEN_RANGE.1);
    assert!(aligned.0.len() >= PrivateTransferArgs::DATA_LEN_RANGE.0);

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

    let view = PrivateTransferArgs::decode(bytes).unwrap();

    assert_eq!(view.shuttle_id(), 100);
    assert_eq!(view.amount(), 200);
    assert_eq!(view.validator(), Some(&[1; 32]));
    assert_eq!(view.encrypted_destination(), &[1, 2, 3, 4]);
    assert_eq!(
        view.encrypted_data_suffix(),
        &[10, 20, 30, 40, 50, 60, 70, 80]
    );

    let encoded = value.encode();
    assert_eq!(encoded, Ok(aligned.0.to_vec()));
    let encoded = encoded.unwrap();

    let mut encoded_out = vec![255; expected_len + 4];
    value.encode_to(&mut encoded_out).unwrap();

    assert_eq!(&encoded_out[..expected_len], &encoded);

    assert_eq!(&encoded_out[expected_len..], &[255, 255, 255, 255]);
}

#[variable_offset_layout(buffer_offset = 0)]
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
    assert_eq!(
        VariableOffsetViewArgs::DATA_LEN_RANGE,
        (14, 2 + (1 + 4) + (1 + 0xFF) + 8 + 2)
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

    let view = VariableOffsetViewArgs::decode(bytes).unwrap();

    assert_eq!(view.header(), 7);
    assert_eq!(view.validator(), Some(9));
    assert_eq!(view.payload(), &[1, 2, 3]);
    assert_eq!(view.amount(), 77);
    assert_eq!(view.checksum(), 0xBEEF);
}

#[test]
fn variable_layout_encode_supports_fields_after_variable_fields() {
    let value = VariableOffsetViewArgs {
        header: 7,
        validator: Some(9),
        payload: vec![1, 2, 3],
        amount: 77,
        checksum: 0xBEEF,
    };

    let encoded = value.encode().unwrap();
    assert_eq!(
        encoded,
        [
            7_u16.to_le_bytes().as_slice(),
            &[1],
            9_u32.to_le_bytes().as_slice(),
            &[3],
            &[1, 2, 3],
            77_u64.to_le_bytes().as_slice(),
            0xBEEF_u16.to_le_bytes().as_slice(),
        ]
        .concat()
    );

    let mut encoded_out = [255; 24];
    value.encode_to(&mut encoded_out).unwrap();
    assert_eq!(&encoded_out[..encoded.len()], &encoded);
    assert_eq!(&encoded_out[encoded.len()..], &[255, 255, 255]);
}

#[test]
fn variable_layout_handles_none_and_empty_vec_before_trailing_fields() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::DATA_LEN_RANGE.0]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&5_u16.to_le_bytes());
    bytes[2] = 0;
    bytes[3] = 0;
    bytes[4..12].copy_from_slice(&55_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&9_u16.to_le_bytes());

    let view = VariableOffsetViewArgs::decode(bytes).unwrap();

    assert_eq!(view.header(), 5);
    assert_eq!(view.validator(), None);
    assert_eq!(view.payload(), &[]);
    assert_eq!(view.amount(), 55);
    assert_eq!(view.checksum(), 9);
}

#[test]
fn variable_layout_encode_minimal_case_with_trailing_fields() {
    let value = VariableOffsetViewArgs {
        header: 5,
        validator: None,
        payload: vec![],
        amount: 55,
        checksum: 9,
    };

    let encoded = value.encode().unwrap();
    assert_eq!(encoded.len(), VariableOffsetViewArgs::DATA_LEN_RANGE.0);
    assert_eq!(
        encoded,
        [
            5_u16.to_le_bytes().as_slice(),
            &[0],
            &[0],
            55_u64.to_le_bytes().as_slice(),
            9_u16.to_le_bytes().as_slice(),
        ]
        .concat()
    );
}

#[test]
fn variable_layout_encode_to_rejects_small_output_buffer() {
    let value = VariableOffsetViewArgs {
        header: 7,
        validator: Some(9),
        payload: vec![1, 2, 3],
        amount: 77,
        checksum: 0xBEEF,
    };

    let mut out = [0_u8; 20];
    assert_eq!(
        value.encode_to(&mut out).unwrap_err(),
        ProgramError::AccountDataTooSmall
    );
}

#[test]
fn variable_layout_encode_rejects_vec_len_that_exceeds_len_width() {
    let value = PrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![0; 256],
        encrypted_data_suffix: vec![],
    };

    assert_eq!(value.encode().unwrap_err(), ProgramError::InvalidRealloc);
}

#[test]
fn variable_layout_try_view_from_rejects_invalid_option_tag() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::DATA_LEN_RANGE.0]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&1_u16.to_le_bytes());
    bytes[2] = 2;
    bytes[3] = 0;
    bytes[4..12].copy_from_slice(&11_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&13_u16.to_le_bytes());

    assert_eq!(
        VariableOffsetViewArgs::decode(bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[test]
fn variable_layout_try_view_from_rejects_truncated_vec_payload() {
    let mut aligned = Aligned([0; VariableOffsetViewArgs::DATA_LEN_RANGE.0]);
    let bytes = &mut aligned.0;

    bytes[0..2].copy_from_slice(&1_u16.to_le_bytes());
    bytes[2] = 0;
    bytes[3] = 11;
    bytes[4..12].copy_from_slice(&11_u64.to_le_bytes());
    bytes[12..14].copy_from_slice(&13_u16.to_le_bytes());

    assert_eq!(
        VariableOffsetViewArgs::decode(bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[variable_offset_layout(buffer_offset = 0)]
struct BorrowedAfterStableVariableArgs {
    pad: [u8; 7],
    #[flexible = 1]
    prefix: Vec<u64>,
    values: [u64; 2],
}

#[test]
fn variable_layout_allows_borrowed_fields_after_stably_aligned_variable_data() {
    let mut aligned = Aligned([0; 40]);
    let bytes = &mut aligned.0;

    bytes[0..7].copy_from_slice(&[9; 7]);
    bytes[7] = 2;
    bytes[8..24].copy_from_slice(&[10, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0]);
    bytes[24..40].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);

    let view = BorrowedAfterStableVariableArgs::decode(bytes).unwrap();

    assert_eq!(view.pad(), [9; 7]);
    assert_eq!(view.prefix(), &[10, 11]);
    let _: &[u64; 2] = view.values();
    assert_eq!(view.values(), &[1, 2]);
}

#[test]
fn variable_layout_rejects_misaligned_base_buffer_for_borrowed_fields() {
    let mut aligned = Aligned([0; 41]);
    let bytes = &mut aligned.0;

    bytes[1..8].copy_from_slice(&[9; 7]);
    bytes[8] = 2;
    bytes[9..25].copy_from_slice(&[10, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0]);
    bytes[25..41].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);

    assert_eq!(
        BorrowedAfterStableVariableArgs::decode(&bytes[1..]).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[variable_offset_layout(buffer_offset = 1)]
struct UnalignedCopyArgs {
    amount: u64,
    counter: u32,
}

#[test]
fn variable_layout_buffer_offset_one_allows_unaligned_copy_only_views() {
    let mut aligned = Aligned([0; 13]);
    let bytes = &mut aligned.0;
    bytes[1..9].copy_from_slice(&55_u64.to_le_bytes());
    bytes[9..13].copy_from_slice(&7_u32.to_le_bytes());

    let view = UnalignedCopyArgs::decode(&bytes[1..]).unwrap();
    assert_eq!(view.amount(), 55);
    assert_eq!(view.counter(), 7);
}

#[variable_offset_layout(buffer_offset = 0)]
struct BoolArgs {
    enabled: bool,
    sponsored: Option<bool>,
    amount: u16,
}

#[test]
fn variable_layout_supports_bool_and_tagged_option_bool() {
    assert_eq!(BoolArgs::DATA_LENS, [4, 5]);

    let none_bytes = [2, 0, 9, 0].to_vec();
    let none_view = BoolArgs::decode(&none_bytes).unwrap();
    assert_eq!(none_view.enabled(), true);
    assert_eq!(none_view.sponsored(), None);
    assert_eq!(none_view.amount(), 9);

    let some_bytes = [7, 1, 3, 9, 0].to_vec();
    let some_view = BoolArgs::decode(&some_bytes).unwrap();
    assert_eq!(some_view.enabled(), true);
    assert_eq!(some_view.sponsored(), Some(true));
    assert_eq!(some_view.amount(), 9);

    let value = BoolArgs {
        enabled: true,
        sponsored: Some(true),
        amount: 9,
    };
    assert_eq!(value.encode().unwrap(), vec![1, 1, 1, 9, 0]);
}

#[variable_offset_layout(buffer_offset = 0, option = implicit)]
struct ImplicitOptionArgs {
    shuttle_id: u32,
    validator: Option<[u8; 32]>,
    amount: u64,
}

#[test]
fn variable_layout_supports_implicit_option_without_tag() {
    assert_eq!(ImplicitOptionArgs::DATA_LENS, [12, 44]);

    let none_value = ImplicitOptionArgs {
        shuttle_id: 100,
        amount: 200,
        validator: None,
    };
    let some_value = ImplicitOptionArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
    };

    let none_encoded = none_value.encode().unwrap();
    assert_eq!(none_encoded.len(), 12);
    assert_eq!(
        none_encoded,
        [
            100_u32.to_le_bytes().as_slice(),
            200_u64.to_le_bytes().as_slice(),
        ]
        .concat()
    );

    let some_encoded = some_value.encode().unwrap();
    assert_eq!(some_encoded.len(), 44);
    assert_eq!(
        some_encoded,
        [
            100_u32.to_le_bytes().as_slice(),
            &[1; 32],
            200_u64.to_le_bytes().as_slice(),
        ]
        .concat()
    );

    let none_view = ImplicitOptionArgs::decode(&none_encoded).unwrap();
    assert_eq!(none_view.shuttle_id(), 100);
    assert_eq!(none_view.amount(), 200);
    assert_eq!(none_view.validator(), None);

    let some_view = ImplicitOptionArgs::decode(&some_encoded).unwrap();
    assert_eq!(some_view.shuttle_id(), 100);
    assert_eq!(some_view.amount(), 200);
    assert_eq!(some_view.validator(), Some(&[1; 32]));
}

#[variable_offset_layout(buffer_offset = 0, option = implicit)]
struct ImplicitOptionWithTrailingArgs {
    header: u16,
    validator: Option<[u8; 4]>,
    amount: u64,
    checksum: u16,
}

#[test]
fn variable_layout_computes_offsets_after_implicit_option() {
    assert_eq!(ImplicitOptionWithTrailingArgs::DATA_LENS, [12, 16]);

    let none_bytes = [
        7_u16.to_le_bytes().as_slice(),
        55_u64.to_le_bytes().as_slice(),
        0xBEEF_u16.to_le_bytes().as_slice(),
    ]
    .concat();
    let none_view = ImplicitOptionWithTrailingArgs::decode(&none_bytes).unwrap();
    assert_eq!(none_view.header(), 7);
    assert_eq!(none_view.validator(), None);
    assert_eq!(none_view.amount(), 55);
    assert_eq!(none_view.checksum(), 0xBEEF);

    let some_bytes = [
        7_u16.to_le_bytes().as_slice(),
        &[9, 8, 7, 6],
        55_u64.to_le_bytes().as_slice(),
        0xBEEF_u16.to_le_bytes().as_slice(),
    ]
    .concat();
    let some_view = ImplicitOptionWithTrailingArgs::decode(&some_bytes).unwrap();
    assert_eq!(some_view.header(), 7);
    assert_eq!(some_view.validator(), Some([9, 8, 7, 6]));
    assert_eq!(some_view.amount(), 55);
    assert_eq!(some_view.checksum(), 0xBEEF);
}

#[test]
fn variable_layout_rejects_invalid_implicit_option_length() {
    let bytes = [
        7_u16.to_le_bytes().as_slice(),
        &[1, 2, 3, 4],
        55_u64.to_le_bytes().as_slice(),
    ]
    .concat();

    assert_eq!(
        ImplicitOptionWithTrailingArgs::decode(&bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[variable_offset_layout(buffer_offset = 0, option = implicit)]
struct MultiImplicitOptionArgs {
    amount: u64,
    split: u32,
    flags: Option<u8>,
    client_ref_id: Option<u64>,
}

#[test]
fn variable_layout_supports_multiple_implicit_options_with_unique_subset_sums() {
    assert_eq!(MultiImplicitOptionArgs::DATA_LENS, [12, 13, 20, 21]);

    let none = MultiImplicitOptionArgs {
        amount: 11,
        split: 2,
        flags: None,
        client_ref_id: None,
    };
    let flags_only = MultiImplicitOptionArgs {
        amount: 11,
        split: 2,
        flags: Some(7),
        client_ref_id: None,
    };
    let client_ref_id_only = MultiImplicitOptionArgs {
        amount: 11,
        split: 2,
        flags: None,
        client_ref_id: Some(99),
    };
    let both = MultiImplicitOptionArgs {
        amount: 11,
        split: 2,
        flags: Some(7),
        client_ref_id: Some(99),
    };

    assert_eq!(none.encode().unwrap().len(), 12);
    assert_eq!(flags_only.encode().unwrap().len(), 13);
    assert_eq!(client_ref_id_only.encode().unwrap().len(), 20);
    assert_eq!(both.encode().unwrap().len(), 21);

    let none_encoded = none.encode().unwrap();
    let none_view = MultiImplicitOptionArgs::decode(&none_encoded).unwrap();
    assert_eq!(none_view.amount(), 11);
    assert_eq!(none_view.split(), 2);
    assert_eq!(none_view.flags(), None);
    assert_eq!(none_view.client_ref_id(), None);

    let flags_only_encoded = flags_only.encode().unwrap();
    let flags_only_view = MultiImplicitOptionArgs::decode(&flags_only_encoded).unwrap();
    assert_eq!(flags_only_view.flags(), Some(7));
    assert_eq!(flags_only_view.client_ref_id(), None);

    let client_ref_id_only_encoded = client_ref_id_only.encode().unwrap();
    let client_ref_id_only_view =
        MultiImplicitOptionArgs::decode(&client_ref_id_only_encoded).unwrap();
    assert_eq!(client_ref_id_only_view.flags(), None);
    assert_eq!(client_ref_id_only_view.client_ref_id(), Some(99));

    let both_encoded = both.encode().unwrap();
    let both_view = MultiImplicitOptionArgs::decode(&both_encoded).unwrap();
    assert_eq!(both_view.flags(), Some(7));
    assert_eq!(both_view.client_ref_id(), Some(99));
}

#[variable_offset_layout(buffer_offset = 0, option = implicit)]
struct ImplicitBoolArgs {
    amount: u16,
    gasless: Option<bool>,
    split: u8,
}

#[test]
fn variable_layout_supports_implicit_option_bool() {
    assert_eq!(ImplicitBoolArgs::DATA_LENS, [3, 4]);

    let none_value = ImplicitBoolArgs {
        amount: 11,
        gasless: None,
        split: 2,
    };
    let some_value = ImplicitBoolArgs {
        amount: 11,
        gasless: Some(true),
        split: 2,
    };

    assert_eq!(none_value.encode().unwrap(), vec![11, 0, 2]);
    assert_eq!(some_value.encode().unwrap(), vec![11, 0, 1, 2]);

    let some_nonzero_bytes = vec![11, 0, 9, 2];
    let some_view = ImplicitBoolArgs::decode(&some_nonzero_bytes).unwrap();
    assert_eq!(some_view.amount(), 11);
    assert_eq!(some_view.gasless(), Some(true));
    assert_eq!(some_view.split(), 2);
}
