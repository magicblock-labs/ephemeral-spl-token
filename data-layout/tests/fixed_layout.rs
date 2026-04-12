use data_layout::fixed_layout;
use pinocchio::error::ProgramError;

#[fixed_layout]
struct DepositAndDelegateShuttleWithPrivateTransferArgs {
    shuttle_id: u32,
    amount: u64,
    validator: Option<[u8; 32]>,
    #[capacity = 72]
    encrypted_destination: Vec<u8>,
    #[capacity = 120]
    encrypted_data_suffix: Vec<u8>,
}

#[test]
fn fixed_layout_private_args() {
    let value = DepositAndDelegateShuttleWithPrivateTransferArgs {
        shuttle_id: 100,
        amount: 200,
        validator: Some([1; 32]),
        encrypted_destination: vec![1, 2, 3, 4],
        encrypted_data_suffix: vec![10, 20, 30, 40, 50, 60, 70, 80],
    };

    let mut bytes = [0; DepositAndDelegateShuttleWithPrivateTransferArgs::DATA_LEN];

    // shuttle_id: u32 (offset: 0)
    bytes[0..4].copy_from_slice(&100_u32.to_le_bytes());

    // amount: u64 (offset: 4)
    bytes[4..12].copy_from_slice(&200_u64.to_le_bytes());

    // validator: Option<[u8; 32]> (offset: 12)
    bytes[12] = 1;
    bytes[13..45].copy_from_slice(&[1; 32]);

    // encrypted_destination: Vec<u8> (offset: 45, capacity = 72)
    bytes[45..53].copy_from_slice(&4_u64.to_le_bytes());
    bytes[53..57].copy_from_slice(&[1, 2, 3, 4]);

    // encrypted_data_suffix: Vec<8> (offset: 125, capacity = 120)
    bytes[125..133].copy_from_slice(&8_u64.to_le_bytes());
    bytes[133..141].copy_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80]);

    let view = DepositAndDelegateShuttleWithPrivateTransferArgs::try_view_from(&bytes).unwrap();

    assert_eq!(view.shuttle_id(), 100);
    assert_eq!(view.amount(), 200);
    assert_eq!(view.validator(), Some(&[1; 32]));
    assert_eq!(view.encrypted_destination(), &[1, 2, 3, 4]);
    assert_eq!(
        view.encrypted_data_suffix(),
        &[10, 20, 30, 40, 50, 60, 70, 80]
    );

    assert_eq!(value.encode(), Ok(bytes));
}

#[fixed_layout]
struct FixedLargeElements {
    #[capacity = 2]
    validators: Vec<[u8; 9]>,
}

#[test]
fn fixed_layout_large_vec_elements_are_borrowed() {
    let mut bytes = vec![0; FixedLargeElements::DATA_LEN];
    bytes[0..8].copy_from_slice(&1_u64.to_le_bytes());
    bytes[8..17].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);

    let view = FixedLargeElements::try_view_from(&bytes).unwrap();
    let _: &[[u8; 9]] = view.validators();
    assert_eq!(view.validators(), &[[1, 2, 3, 4, 5, 6, 7, 8, 9]]);
}

#[fixed_layout]
struct FixedReadonlyArgs {
    amount: u64,
    validator: Option<[u8; 32]>,
    client_ref_id: Option<u64>,
    _pad0: [u8; 2],
    #[capacity = 10]
    recipients: Vec<u16>,
}

#[test]
fn fixed_layout_reserves_constant_space() {
    assert_eq!(FixedReadonlyArgs::DATA_LEN, 80);

    let mut bytes = vec![0; FixedReadonlyArgs::DATA_LEN];
    bytes[0..8].copy_from_slice(&9_u64.to_le_bytes());
    bytes[8] = 1;
    bytes[9..41].copy_from_slice(&[7_u8; 32]);
    bytes[41] = 1;
    bytes[42..50].copy_from_slice(&55_u64.to_le_bytes());

    bytes[52..60].copy_from_slice(&2_u64.to_le_bytes());
    bytes[60..62].copy_from_slice(&11_u16.to_le_bytes());
    bytes[62..64].copy_from_slice(&13_u16.to_le_bytes());

    let view = FixedReadonlyArgs::try_view_from(&bytes).unwrap();
    assert_eq!(view.amount(), 9);
    let _: Option<&[u8; 32]> = view.validator();
    assert_eq!(view.validator(), Some(&[7_u8; 32]));
    assert_eq!(view.client_ref_id(), Some(55));

    let _: &[u16] = view.recipients();
    assert_eq!(view.recipients_capacity(), 10);
    assert_eq!(view.recipients(), &[11, 13]);
}

#[test]
fn fixed_layout_rejects_invalid_vec_len() {
    let mut bytes = vec![0; FixedReadonlyArgs::DATA_LEN];
    bytes[0..8].copy_from_slice(&1_u64.to_le_bytes());

    // vec len = 11, where capacity = 10, hence invalid len
    bytes[52..60].copy_from_slice(&11_u64.to_le_bytes());

    assert_eq!(
        FixedReadonlyArgs::try_view_from(&bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}

#[fixed_layout]
struct FixedAlignedBorrowedFields {
    flag: u8,
    _pad0: [u8; 7],
    payload: [u64; 2],
    #[capacity = 2]
    values: Vec<[u64; 2]>,
}

#[test]
fn fixed_layout_borrows_aligned_large_fields() {
    let mut bytes = vec![0; FixedAlignedBorrowedFields::DATA_LEN];
    bytes[0] = 1;

    bytes[8..24].copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);

    bytes[24..32].copy_from_slice(&1_u64.to_le_bytes());
    bytes[32..48].copy_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0]);

    let view = FixedAlignedBorrowedFields::try_view_from(&bytes).unwrap();
    let _: &[u64; 2] = view.payload();
    assert_eq!(view.payload(), &[1, 2]);

    let _: &[[u64; 2]] = view.values();
    assert_eq!(view.values(), &[[3, 4]]);
}
