use data_layout::variable_layout;
use pinocchio::error::ProgramError;

#[variable_layout]
pub struct DelegateArgs {
    validator: Option<[u8; 32]>,
}

#[variable_layout(mut)]
struct DepositAndDelegateShuttleArgs {
    shuttle_id: u32,
    amount: u64,
    validator: Option<[u8; 32]>,
    #[max_len = 8]
    destination: Option<String>,
}

#[variable_layout]
struct ReadonlyArgs {
    amount: u64,
    client_ref_id: Option<[u8; 32]>,
    #[max_len = 8]
    memo: Option<String>,
}

#[test]
fn parses_without_optional_payloads() {
    let mut bytes = vec![0; DepositAndDelegateShuttleArgs::DATA_LEN];
    bytes[0..4].copy_from_slice(&7_u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&99_u64.to_le_bytes());
    bytes[12] = 0;
    bytes[45] = 0;

    let view = DepositAndDelegateShuttleArgs::try_view_from(&bytes).unwrap();
    assert_eq!(view.shuttle_id(), 7);
    assert_eq!(view.amount(), 99);
    assert_eq!(view.validator(), None);
    assert_eq!(view.destination(), None);
}

#[test]
fn parses_multiple_optional_payloads() {
    let mut bytes = vec![0; DepositAndDelegateShuttleArgs::DATA_LEN];
    bytes[0..4].copy_from_slice(&7_u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&99_u64.to_le_bytes());
    bytes[12] = 1;
    bytes[13..45].copy_from_slice(&[5_u8; 32]);
    bytes[45] = 1;
    bytes[46] = 5;
    bytes[47..52].copy_from_slice(b"alice");

    let view = DepositAndDelegateShuttleArgs::try_view_from(&bytes).unwrap();
    assert_eq!(view.shuttle_id(), 7);
    assert_eq!(view.amount(), 99);
    assert_eq!(view.validator(), Some(&[5_u8; 32]));
    assert_eq!(view.destination(), Some("alice"));
}

#[test]
fn mutable_view_can_update_present_fields_without_size_change() {
    let mut bytes = vec![0; DepositAndDelegateShuttleArgs::DATA_LEN];
    bytes[0..4].copy_from_slice(&7_u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&99_u64.to_le_bytes());
    bytes[12] = 1;
    bytes[13..45].copy_from_slice(&[5_u8; 32]);
    bytes[45] = 1;
    bytes[46] = 5;
    bytes[47..52].copy_from_slice(b"alice");

    let mut view = DepositAndDelegateShuttleArgs::try_view_from_mut(&mut bytes).unwrap();
    view.set_shuttle_id(11).unwrap();
    view.set_amount(123).unwrap();
    view.set_validator([9_u8; 32]).unwrap();
    view.set_destination("blake").unwrap();

    assert_eq!(view.shuttle_id(), 11);
    assert_eq!(view.amount(), 123);
    assert_eq!(view.validator(), Some(&[9_u8; 32]));
    assert_eq!(view.destination(), Some("blake"));
}

#[test]
fn mutable_view_rejects_none_to_some_and_size_changes() {
    let mut bytes = vec![0; DepositAndDelegateShuttleArgs::DATA_LEN];
    bytes[0..4].copy_from_slice(&7_u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&99_u64.to_le_bytes());
    bytes[12] = 0;
    bytes[45] = 1;
    bytes[46] = 5;
    bytes[47..52].copy_from_slice(b"alice");

    let mut view = DepositAndDelegateShuttleArgs::try_view_from_mut(&mut bytes).unwrap();
    assert_eq!(
        view.set_validator([9_u8; 32]),
        Err(ProgramError::InvalidInstructionData)
    );
    assert_eq!(
        view.set_destination("bob"),
        Err(ProgramError::InvalidInstructionData)
    );
}

#[test]
fn readonly_view_works() {
    let mut bytes = vec![0; ReadonlyArgs::DATA_LEN];
    bytes[0..8].copy_from_slice(&9_u64.to_le_bytes());
    bytes[8] = 1;
    bytes[9..41].copy_from_slice(&[3_u8; 32]);
    bytes[41] = 1;
    bytes[42] = 4;
    bytes[43..47].copy_from_slice(b"note");

    let view = ReadonlyArgs::try_view_from(&bytes).unwrap();
    assert_eq!(view.amount(), 9);
    assert_eq!(view.client_ref_id(), Some(&[3_u8; 32]));
    assert_eq!(view.memo(), Some("note"));
}

#[test]
fn rejects_invalid_optional_tags() {
    let mut bytes = vec![0; DepositAndDelegateShuttleArgs::DATA_LEN];
    bytes[0..4].copy_from_slice(&7_u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&99_u64.to_le_bytes());
    bytes[12] = 2;
    bytes[45] = 0;

    assert_eq!(
        DepositAndDelegateShuttleArgs::try_view_from(&bytes).unwrap_err(),
        ProgramError::InvalidInstructionData
    );
}
