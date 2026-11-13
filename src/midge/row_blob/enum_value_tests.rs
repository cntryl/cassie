use super::*;
use crate::midge::row_blob::encoding::encode_value;
use crate::types::enum_type::{sort_order_at, ENUM_STORAGE_KEY};
use crate::types::{DataType, EnumMember, EnumRef};

fn mood_type() -> EnumType {
    EnumType {
        oid: 300_001,
        array_oid: 300_002,
        qualified_name: "db.public.mood".to_string(),
        name: "mood".to_string(),
        revision: 3,
        members: ["sad", "ok", "happy"]
            .iter()
            .enumerate()
            .map(|(position, label)| EnumMember {
                member_id: 7_000 + u64::try_from(position).expect("small position"),
                label: (*label).to_string(),
                sort_order: sort_order_at(position),
                uncommitted: false,
            })
            .collect(),
    }
}

fn stored(member_id: u64) -> serde_json::Value {
    serde_json::json!({ ENUM_STORAGE_KEY: member_id })
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    super::super::write_varint(u64::try_from(payload.len()).expect("length"), &mut out);
    out.extend_from_slice(payload);
    out
}

#[test]
fn should_write_the_selected_golden_layout_for_a_scalar() {
    // Arrange
    let descriptor = mood_type();

    // Act
    let (tag, payload) = encode_enum(&descriptor, &stored(7_001)).expect("encode enum");

    // Assert
    assert_eq!(tag, 0x11);
    assert_eq!(
        payload,
        vec![
            1, // version
            0x00, 0x04, 0x93, 0xE1, // type oid 300_001 big-endian
            0, 0, 0, 0, 0, 0, 0x1B, 0x59, // member id 7_001 big-endian
        ]
    );
}

#[test]
fn should_round_trip_a_scalar_to_the_stored_json_form() {
    // Arrange
    let descriptor = mood_type();
    let (_, payload) = encode_enum(&descriptor, &stored(7_002)).expect("encode enum");
    let bytes = framed(&payload);

    // Act
    let decoded = decode_enum(&mut Cursor::new(&bytes)).expect("decode enum");

    // Assert
    assert_eq!(decoded, stored(7_002));
}

#[test]
fn should_use_the_outer_null_marker_for_an_enum_null() {
    // Arrange
    let data_type = DataType::Enum(EnumRef::new(mood_type()));

    // Act
    let (tag, payload) = encode_value(&data_type, &serde_json::Value::Null).expect("encode NULL");

    // Assert
    assert_eq!(tag, TYPE_NULL);
    assert_eq!(payload, Vec::<u8>::new());
}

#[test]
fn should_reject_a_member_the_descriptor_does_not_know_when_encoding() {
    // Arrange
    let descriptor = mood_type();

    // Act
    let unknown = encode_enum(&descriptor, &stored(99));
    let label_text = encode_enum(&descriptor, &serde_json::json!("ok"));

    // Assert
    assert!(unknown.is_err());
    assert!(label_text.is_err());
}

#[test]
fn should_reject_every_truncation_of_an_enum_payload() {
    // Arrange
    let descriptor = mood_type();
    let (_, payload) = encode_enum(&descriptor, &stored(7_000)).expect("encode enum");

    // Act
    let failures = (0..payload.len())
        .filter(|end| decode_payload(&payload[..*end]).is_err())
        .count();

    // Assert
    assert_eq!(failures, payload.len());
}

#[test]
fn should_reject_an_unknown_payload_version_and_trailing_bytes() {
    // Arrange
    let descriptor = mood_type();
    let (_, payload) = encode_enum(&descriptor, &stored(7_000)).expect("encode enum");
    let mut wrong_version = payload.clone();
    wrong_version[0] = 2;
    let mut trailing = payload;
    trailing.push(0);

    // Act
    let version = decode_payload(&wrong_version);
    let extra = decode_payload(&trailing);

    // Assert
    assert!(version.is_err());
    assert!(extra.is_err());
}

#[test]
fn should_report_corruption_for_a_stored_member_the_column_type_lacks() {
    // Arrange
    let data_type = DataType::Enum(EnumRef::new(mood_type()));

    // Act
    let known = check_column(&data_type, &stored(7_002));
    let unknown = check_column(&data_type, &stored(8_000));

    // Assert
    assert!(known.is_ok());
    let error = unknown.expect_err("unknown member id is corruption");
    assert_eq!(error.sql_state(), "XX001");
}

#[test]
fn should_write_the_golden_layout_for_an_enum_array_with_a_null_element() {
    // Arrange
    let element = DataType::Enum(EnumRef::new(mood_type()));
    let data_type = DataType::Array(Box::new(element));
    let value = serde_json::json!([stored(7_000), null, stored(7_002)]);

    // Act
    let (tag, payload) = encode_value(&data_type, &value).expect("encode enum array");

    // Assert
    let scalar = |member: u8| -> Vec<u8> {
        let mut bytes = vec![0x11, 13, 1, 0x00, 0x04, 0x93, 0xE1, 0, 0, 0, 0, 0, 0];
        bytes.push(0x1B);
        bytes.push(0x58 + member);
        bytes
    };
    let mut expected = vec![1]; // one dimension
    expected.extend_from_slice(&3_i32.to_be_bytes());
    expected.extend_from_slice(&1_i32.to_be_bytes());
    expected.push(3); // element count
    expected.extend_from_slice(&scalar(0));
    expected.extend_from_slice(&[0x00, 0]); // NULL element
    expected.extend_from_slice(&scalar(2));
    assert_eq!(tag, 0x0C);
    assert_eq!(payload, expected);
}

#[test]
fn should_round_trip_an_enum_array_and_detect_an_unknown_element() {
    // Arrange
    let descriptor = EnumRef::new(mood_type());
    let data_type = DataType::Array(Box::new(DataType::Enum(descriptor)));
    let value = serde_json::json!([stored(7_001), null]);
    let (_, payload) = encode_value(&data_type, &value).expect("encode enum array");
    let bytes = payload;

    // Act
    let decoded = super::super::decode_array(&mut Cursor::new(&bytes), None).expect("decode");
    let checked = check_column(&data_type, &decoded);
    let mut corrupted = bytes.clone();
    // Byte 24 is the low byte of the first element's member id.
    corrupted[24] = 0x7F;
    let corrupted_decoded =
        super::super::decode_array(&mut Cursor::new(&corrupted), None).expect("decode");
    let corrupted_check = check_column(&data_type, &corrupted_decoded);

    // Assert
    assert!(checked.is_ok());
    assert!(corrupted_check.is_err());
}
