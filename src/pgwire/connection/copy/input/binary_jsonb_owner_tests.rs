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
        let error = match super::parse_binary_payload(&payload, &cassie, &statement, &controls) {
            Ok(_) => panic!("accepted"),
            Err(error) => error,
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
fn should_keep_jsonb_array_on_its_existing_binary_codec_error_path() {
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
        matches!(result, Err(super::super::filter::CopyWhereError::Cassie(crate::app::CassieError::Parse(message))) if message == format!("COPY binary field 1 is invalid: {original}"))
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
