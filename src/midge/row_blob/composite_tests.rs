use super::*;
use crate::types::composite::{CompositeField, CompositeType, RECORD_STORAGE_KEY};
use crate::types::{CompositeRef, DataType};

fn pair_type() -> CompositeType {
    CompositeType {
        oid: 200_001,
        array_oid: 200_002,
        qualified_name: "db.public.pair".to_string(),
        name: "pair".to_string(),
        revision: 7,
        dropped_positions: Vec::new(),
        fields: vec![
            CompositeField {
                name: "key".to_string(),
                data_type: DataType::Text,
            },
            CompositeField {
                name: "value".to_string(),
                data_type: DataType::BigInt,
            },
        ],
    }
}

fn wrap(body: serde_json::Value) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert(RECORD_STORAGE_KEY.to_string(), body);
    serde_json::Value::Object(object)
}

fn record_json(oid: u32, fields: &[serde_json::Value]) -> serde_json::Value {
    wrap(serde_json::json!({ "oid": oid, "revision": 7, "fields": fields }))
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(u64::try_from(payload.len()).expect("length"), &mut out);
    out.extend_from_slice(payload);
    out
}

#[test]
fn should_write_the_selected_header_before_the_framed_fields() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid,
        &[serde_json::json!("a"), serde_json::json!(5)],
    );

    // Act
    let (tag, payload) = encode_composite(&descriptor, &value).expect("encode record");

    // Assert
    assert_eq!(tag, TYPE_COMPOSITE);
    assert_eq!(tag, 0x0F);
    assert_eq!(payload[0], 1);
    assert_eq!(payload[1..5], 200_001_u32.to_be_bytes());
    assert_eq!(payload[5..13], 7_u64.to_be_bytes());
    assert_eq!(payload[13..17], 2_u32.to_be_bytes());
    assert_eq!(payload[17], 0x05, "first field is a framed string");
}

#[test]
fn should_round_trip_a_record_with_a_null_field() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid,
        &[serde_json::json!("a b"), serde_json::Value::Null],
    );
    let (_, payload) = encode_composite(&descriptor, &value).expect("encode record");
    let bytes = framed(&payload);

    // Act
    let decoded = decode_composite(&mut Cursor::new(&bytes), None).expect("decode record");

    // Assert
    assert_eq!(decoded, value);
}

#[test]
fn should_keep_an_all_null_record_distinct_from_a_null_field() {
    // Arrange
    let descriptor = pair_type();
    let all_null = record_json(
        descriptor.oid,
        &[serde_json::Value::Null, serde_json::Value::Null],
    );

    // Act
    let (record_tag, _) = encode_composite(&descriptor, &all_null).expect("encode record");
    let (null_tag, null_payload) = encode_value(
        &DataType::Composite(CompositeRef::new(descriptor)),
        &serde_json::Value::Null,
    )
    .expect("encode NULL");

    // Assert
    assert_eq!(record_tag, TYPE_COMPOSITE);
    assert_eq!(null_tag, TYPE_NULL);
    assert_eq!(null_payload, Vec::<u8>::new());
}

#[test]
fn should_round_trip_a_nested_record() {
    // Arrange
    let inner = pair_type();
    let outer = CompositeType {
        oid: 200_010,
        array_oid: 200_011,
        qualified_name: "db.public.wrapper".to_string(),
        name: "wrapper".to_string(),
        revision: 1,
        dropped_positions: Vec::new(),
        fields: vec![CompositeField {
            name: "inner".to_string(),
            data_type: DataType::Composite(CompositeRef::new(inner.clone())),
        }],
    };
    let nested = record_json(inner.oid, &[serde_json::json!("x"), serde_json::json!(1)]);
    let value = wrap(serde_json::json!({ "oid": outer.oid, "revision": 1, "fields": [nested] }));
    let (_, payload) = encode_composite(&outer, &value).expect("encode nested record");
    let bytes = framed(&payload);

    // Act
    let decoded = decode_composite(&mut Cursor::new(&bytes), None).expect("decode nested");

    // Assert
    assert_eq!(
        decoded[RECORD_STORAGE_KEY]["fields"][0],
        value[RECORD_STORAGE_KEY]["fields"][0]
    );
}

#[test]
fn should_reject_a_record_for_a_different_type() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid + 1,
        &[serde_json::json!("a"), serde_json::json!(5)],
    );

    // Act
    let result = encode_composite(&descriptor, &value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_a_record_with_the_wrong_field_count() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(descriptor.oid, &[serde_json::json!("a")]);

    // Act
    let result = encode_composite(&descriptor, &value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_every_truncation_of_a_composite_payload() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid,
        &[serde_json::json!("abc"), serde_json::json!(9)],
    );
    let (_, payload) = encode_composite(&descriptor, &value).expect("encode record");

    // Act
    let failures = (0..payload.len())
        .filter(|end| decode_payload(&payload[..*end], None, 0).is_err())
        .count();

    // Assert
    assert_eq!(failures, payload.len());
}

#[test]
fn should_reject_an_unknown_payload_version() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid,
        &[serde_json::json!("a"), serde_json::json!(5)],
    );
    let (_, mut payload) = encode_composite(&descriptor, &value).expect("encode record");
    payload[0] = 2;

    // Act
    let result = decode_payload(&payload, None, 0);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_trailing_bytes_after_the_last_field() {
    // Arrange
    let descriptor = pair_type();
    let value = record_json(
        descriptor.oid,
        &[serde_json::json!("a"), serde_json::json!(5)],
    );
    let (_, mut payload) = encode_composite(&descriptor, &value).expect("encode record");
    payload.push(0);

    // Act
    let result = decode_payload(&payload, None, 0);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_a_field_count_larger_than_the_payload() {
    // Arrange
    let mut payload = vec![COMPOSITE_PAYLOAD_VERSION];
    payload.extend_from_slice(&5_u32.to_be_bytes());
    payload.extend_from_slice(&1_u64.to_be_bytes());
    payload.extend_from_slice(&u32::MAX.to_be_bytes());

    // Act
    let result = decode_payload(&payload, None, 0);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_payloads_nested_beyond_the_depth_limit() {
    // Arrange
    let mut payload = vec![COMPOSITE_PAYLOAD_VERSION];
    payload.extend_from_slice(&5_u32.to_be_bytes());
    payload.extend_from_slice(&1_u64.to_be_bytes());
    payload.extend_from_slice(&0_u32.to_be_bytes());
    for _ in 0..=MAX_COMPOSITE_DEPTH {
        let mut wrapper = vec![COMPOSITE_PAYLOAD_VERSION];
        wrapper.extend_from_slice(&5_u32.to_be_bytes());
        wrapper.extend_from_slice(&1_u64.to_be_bytes());
        wrapper.extend_from_slice(&1_u32.to_be_bytes());
        wrapper.push(TYPE_COMPOSITE);
        wrapper.extend_from_slice(&framed(&payload));
        payload = wrapper;
    }

    // Act
    let result = decode_payload(&payload, None, 0);

    // Assert
    assert!(result.is_err());
}
