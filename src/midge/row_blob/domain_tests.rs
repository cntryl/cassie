use super::*;
use crate::types::DataType;

fn posint() -> DomainType {
    DomainType {
        oid: 200_101,
        array_oid: 200_102,
        qualified_name: "db.public.posint".to_string(),
        name: "posint".to_string(),
        revision: 3,
        base: DataType::BigInt,
        not_null: false,
        default: None,
        checks: Vec::new(),
    }
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(u64::try_from(payload.len()).expect("length"), &mut out);
    out.extend_from_slice(payload);
    out
}

#[test]
fn should_write_the_selected_header_before_the_framed_base_value() {
    // Arrange
    let descriptor = posint();

    // Act
    let (tag, payload) = encode_domain(&descriptor, &serde_json::json!(5)).expect("encode");

    // Assert
    assert_eq!(tag, TYPE_DOMAIN);
    assert_eq!(tag, 0x10);
    assert_eq!(payload[0], 1);
    assert_eq!(payload[1..5], 200_101_u32.to_be_bytes());
    assert_eq!(payload[5..13], 3_u64.to_be_bytes());
    assert_eq!(payload[13], TYPE_I64);
}

#[test]
fn should_round_trip_a_domain_value_as_its_base_value() {
    // Arrange
    let descriptor = posint();
    let (_, payload) = encode_domain(&descriptor, &serde_json::json!(42)).expect("encode");
    let bytes = framed(&payload);

    // Act
    let decoded = decode_domain(&mut Cursor::new(&bytes), None).expect("decode");

    // Assert
    assert_eq!(decoded, serde_json::json!(42));
}

#[test]
fn should_reject_a_domain_payload_with_an_unknown_version() {
    // Arrange
    let descriptor = posint();
    let (_, mut payload) = encode_domain(&descriptor, &serde_json::json!(1)).expect("encode");
    payload[0] = DOMAIN_PAYLOAD_VERSION + 1;
    let bytes = framed(&payload);

    // Act
    let decoded = decode_domain(&mut Cursor::new(&bytes), None);

    // Assert
    assert!(decoded.is_err(), "unknown payload versions fail closed");
}

#[test]
fn should_reject_a_domain_payload_that_wraps_a_null_base_value() {
    // Arrange
    let descriptor = posint();
    let (_, mut payload) = encode_domain(&descriptor, &serde_json::json!(1)).expect("encode");
    payload.truncate(13);
    payload.push(TYPE_NULL);
    let bytes = framed(&payload);

    // Act
    let decoded = decode_domain(&mut Cursor::new(&bytes), None);

    // Assert
    assert!(decoded.is_err(), "a NULL base would bypass NOT NULL");
}

#[test]
fn should_refuse_to_encode_a_null_base_value() {
    // Arrange
    let descriptor = posint();

    // Act
    let encoded = encode_domain(&descriptor, &serde_json::Value::Null);

    // Assert
    assert!(encoded.is_err(), "domain NULL uses the outer NULL marker");
}
