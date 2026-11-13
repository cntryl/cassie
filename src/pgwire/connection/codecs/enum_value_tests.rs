use super::{decode_labels, encode};
use crate::executor::EnumColumn;
use crate::types::{SqlArray, Value};

const ELEMENT_OID: i64 = 20_001;

fn scalar() -> EnumColumn {
    EnumColumn {
        element_oid: ELEMENT_OID,
        is_array: false,
    }
}

fn array() -> EnumColumn {
    EnumColumn {
        element_oid: ELEMENT_OID,
        is_array: true,
    }
}

fn labels(values: &[&str]) -> Value {
    let elements = values
        .iter()
        .map(|label| Value::String((*label).to_string()))
        .collect();
    Value::Array(SqlArray::from_elements(elements).expect("array"))
}

#[test]
fn should_send_an_enum_label_as_utf8_in_text_and_binary_formats() {
    // Arrange
    let value = Value::String("CA".to_string());

    // Act
    let text = encode(&value, scalar(), 0).expect("text");
    let binary = encode(&value, scalar(), 1).expect("binary");

    // Assert
    assert_eq!(text, b"CA");
    assert_eq!(binary, b"CA");
}

#[test]
fn should_reject_an_enum_cell_that_is_not_a_label_or_an_unknown_format() {
    // Arrange
    let number = Value::Int64(1);
    let label = Value::String("CA".to_string());

    // Act
    let not_a_label = encode(&number, scalar(), 1);
    let bad_format = encode(&label, scalar(), 2);

    // Assert
    assert!(not_a_label.is_err());
    assert!(bad_format.is_err());
}

#[test]
fn should_render_an_enum_array_in_array_out_text_with_quoting() {
    // Arrange
    let value = labels(&["CA", "has space", "NULL"]);

    // Act
    let text = encode(&value, array(), 0).expect("text");

    // Assert
    assert_eq!(text, br#"{CA,"has space","NULL"}"#);
}

#[test]
fn should_send_an_enum_array_with_the_enum_element_oid_and_label_bytes() {
    // Arrange
    let value = labels(&["CA", "WA"]);
    let mut expected = Vec::new();
    for word in [1_i32, 0, 20_001, 2, 1] {
        expected.extend_from_slice(&word.to_be_bytes());
    }
    for label in ["CA", "WA"] {
        expected.extend_from_slice(&2_i32.to_be_bytes());
        expected.extend_from_slice(label.as_bytes());
    }

    // Act
    let binary = encode(&value, array(), 1).expect("binary");

    // Assert
    assert_eq!(binary, expected);
}

#[test]
fn should_round_trip_an_enum_array_through_binary_labels() {
    // Arrange
    let value = labels(&["CA", "WA"]);
    let binary = encode(&value, array(), 1).expect("binary");

    // Act
    let decoded = decode_labels(&binary, array()).expect("decode");

    // Assert
    assert_eq!(decoded, value);
}

#[test]
fn should_decode_a_binary_scalar_enum_parameter_as_its_label() {
    // Arrange
    let bytes = "WA".as_bytes();

    // Act
    let decoded = decode_labels(bytes, scalar()).expect("decode");
    let invalid = decode_labels(&[0xff, 0xfe], scalar());

    // Assert
    assert_eq!(decoded, Value::String("WA".to_string()));
    assert!(invalid.is_err());
}

#[test]
fn should_reject_a_binary_enum_array_with_a_foreign_element_oid() {
    // Arrange
    let mut bytes = Vec::new();
    for word in [0_i32, 0, 25] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }

    // Act
    let decoded = decode_labels(&bytes, array());

    // Assert
    assert!(decoded.is_err());
}
