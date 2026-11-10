use std::time::Instant;

use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;

#[test]
fn should_keep_jsonb_copy_diagnostic_owned_with_original_message() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_jsonb_owner (value JSONB)",
            vec![],
        )
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_jsonb_owner (value) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let fields: &[&[u8]] = &[
        &[],
        &[2, 255],
        &[1, 255],
        b"\x01[1,]",
        b"\x011e+131072",
        b"\x01\"\\u0000\"",
    ];
    // Act / Assert
    for field in fields {
        let original =
            super::super::super::codecs::binary_to_value(field, 3802).expect_err("invalid");
        let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
        payload.extend_from_slice(field);
        payload.extend_from_slice(&(-1_i16).to_be_bytes());
        let Err(error) = super::parse_binary_payload(&payload, &cassie, &statement, &controls)
        else {
            panic!("accepted");
        };
        let super::super::filter::CopyWhereError::Jsonb(owner) = error else {
            panic!("owned JSONB error")
        };
        assert_eq!(
            owner.as_str(),
            format!("COPY binary field 1 is invalid: {original}")
        );
        assert!(controls.current_query_memory_bytes() >= owner.as_str().len());
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_jsonb_array_binary_codec_error_text_in_owned_copy_error() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_jsonb_array_owner (value JSONB[])",
            vec![],
        )
        .expect("table");
    let parsed =
        crate::sql::parser::parse_statement("COPY copy_jsonb_array_owner (value) FROM STDIN")
            .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut array = Vec::new();
    for value in [1_i32, 0, 3802, 1, 1] {
        array.extend_from_slice(&value.to_be_bytes());
    }
    array.extend_from_slice(&5_i32.to_be_bytes());
    array.extend_from_slice(b"\x01[1,]");
    let original = super::super::super::codecs::binary_to_value(&array, 3807).expect_err("invalid");
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&i32::try_from(array.len()).expect("length").to_be_bytes());
    payload.extend_from_slice(&array);
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    // Act
    let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
    // Assert
    assert!(
        matches!(result, Err(super::super::filter::CopyWhereError::Jsonb(owner)) if owner.as_str() == format!("COPY binary field 1 is invalid: {original}"))
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_legacy_jsonb_array_envelope_and_element_error_precedence() {
    use crate::runtime::accounted::json_arena::jsonb_array_envelope::{decode, DecodeError};
    use crate::types::Value;

    fn make(
        length: i32,
        lower: i32,
        has_null: i32,
        fields: &[Option<&[u8]>],
        trailer: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [1_i32, has_null, 3802, length, lower] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for field in fields {
            if let Some(field) = field {
                bytes.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
                bytes.extend_from_slice(field);
            } else {
                bytes.extend_from_slice(&(-1_i32).to_be_bytes());
            }
        }
        bytes.extend_from_slice(trailer);
        bytes
    }
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut empty = Vec::new();
    for value in [0_i32, 0, 3802] {
        empty.extend_from_slice(&value.to_be_bytes());
    }
    let mut multi = Vec::new();
    for value in [2_i32, 0, 3802, 1, 1, 2, -1] {
        multi.extend_from_slice(&value.to_be_bytes());
    }
    for field in [b"\x011".as_slice(), b"\x012".as_slice()] {
        multi.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
        multi.extend_from_slice(field);
    }
    let cases = [
        Vec::new(),
        empty,
        multi,
        make(0, 1, 0, &[], &[]),
        make(-1, 1, 0, &[], &[]),
        make(2, 1, 0, &[Some(b"\x01[1,]")], &[]),
        make(2, 1, 0, &[Some(b"\x01[1,]"), Some(b"\x01null")], &[42]),
        make(2, 1, 0, &[Some(b"\x011"), Some(b"\x01[1,]")], &[42]),
        make(1, 1, 0, &[None], &[]),
        make(1, 1, 0, &[Some(b"\x01null")], &[]),
        make(1, i32::MAX, 0, &[Some(b"\x01{\"z\":1}")], &[]),
    ];
    // Act / Assert
    for input in cases {
        let expected = super::super::super::codecs::binary_to_value(&input, 3807);
        let actual = decode(
            &input,
            &controls,
            |field| {
                let value = super::super::super::codecs::binary_to_value(field, 3802)?;
                Ok::<_, std::io::Error>(matches!(value, Value::Json(value) if value.is_null()))
            },
            |_| {},
        );
        match (expected, actual) {
            (Ok(_), Ok(owner)) => drop(owner),
            (Err(old), Err(new)) => {
                let message = match new {
                    DecodeError::InvalidArray => "invalid binary array value".to_owned(),
                    DecodeError::DocumentNull => {
                        "binary format is not supported for array OID 37802".to_owned()
                    }
                    DecodeError::Element(error) => error.to_string(),
                    DecodeError::Control(error) => panic!("unexpected control: {error}"),
                };
                assert_eq!(message, old.to_string());
            }
            _ => panic!("array codec disagreement"),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_legacy_jsonb_array_text_and_compact_fallback() {
    use crate::runtime::accounted::json_arena::jsonb_array_output::render;

    fn make(dimensions: &[(i32, i32)], fields: &[Option<&[u8]>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let has_null = i32::from(fields.iter().any(Option::is_none));
        for value in [
            i32::try_from(dimensions.len()).expect("dimensions"),
            has_null,
            3802,
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for (length, lower) in dimensions {
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(&lower.to_be_bytes());
        }
        for field in fields {
            if let Some(field) = field {
                bytes.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
                bytes.extend_from_slice(field);
            } else {
                bytes.extend_from_slice(&(-1_i32).to_be_bytes());
            }
        }
        bytes
    }
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        make(&[], &[]),
        make(
            &[(3, 1)],
            &[Some(b"\x011.00"), None, Some(b"\x01{\"z\":1,\"a\":2}")],
        ),
        make(
            &[(1, -2), (2, 1)],
            &[Some(b"\x01\"q\\\"x\""), Some(b"\x01[1,2]")],
        ),
        make(&[(1, i32::MAX)], &[Some(b"\x01{\"aa\":1,\"z\":2}")]),
    ];
    // Act / Assert
    for input in cases {
        let value =
            super::super::super::codecs::binary_to_value(&input, 3807).expect("legacy value");
        let expected = super::super::super::codecs::value_to_text(&value, 3807);
        let actual = render(&input, 1, &controls).expect("controlled output");
        assert_eq!(actual.as_str(), expected);
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_return_owned_jsonb_array_text_from_binary_copy() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_jsonb_array_output (value JSONB[])",
            vec![],
        )
        .expect("table");
    let parsed =
        crate::sql::parser::parse_statement("COPY copy_jsonb_array_output (value) FROM STDIN")
            .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut array = Vec::new();
    for value in [1_i32, 0, 3802, 1, i32::MAX] {
        array.extend_from_slice(&value.to_be_bytes());
    }
    let field = b"\x01{\"aa\":1,\"z\":2}";
    array.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
    array.extend_from_slice(field);
    let original = super::super::super::codecs::binary_to_value(&array, 3807).expect("legacy");
    let expected = super::super::super::codecs::value_to_text(&original, 3807);
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&i32::try_from(array.len()).expect("length").to_be_bytes());
    payload.extend_from_slice(&array);
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    // Act
    let (rows, memory) =
        super::parse_binary_payload(&payload, &cassie, &statement, &controls).expect("copy");
    // Assert
    assert_eq!(rows, vec![vec![Some(expected)]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(rows);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
