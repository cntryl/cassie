use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;

fn copy_payload(field: &[u8]) -> Vec<u8> {
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
    payload.extend_from_slice(field);
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    payload
}

fn array_field(oid: i32, width: usize, value: &[u8]) -> Vec<u8> {
    let mut array = Vec::new();
    for number in [1_i32, 0, oid, 2, 1] {
        array.extend_from_slice(&number.to_be_bytes());
    }
    array.extend_from_slice(&i32::try_from(width).expect("width").to_be_bytes());
    array.extend(std::iter::repeat_n(0, width));
    array.extend_from_slice(&i32::try_from(value.len()).expect("length").to_be_bytes());
    array.extend_from_slice(value);
    array
}

#[test]
fn should_preserve_date_time_binary_errors_in_scalar_and_array_copy() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let types = [
        (
            "date",
            "DATE",
            1082,
            4,
            (-2_451_546_i32).to_be_bytes().to_vec(),
        ),
        ("time", "TIME", 1083, 8, (-1_i64).to_be_bytes().to_vec()),
    ];
    for (label, sql_type, element_oid, width, invalid) in types {
        for array in [false, true] {
            let suffix = if array { "[]" } else { "" };
            let name = format!("{label}_{}_bounds", if array { "array" } else { "scalar" });
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {name} (value {sql_type}{suffix})"),
                    vec![],
                )
                .expect("table");
            let parsed =
                crate::sql::parser::parse_statement(&format!("COPY {name} (value) FROM STDIN"))
                    .expect("parse");
            let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
                panic!("COPY")
            };
            let extra = [vec![0; width], vec![0]].concat();
            let invalid_extra = [invalid.as_slice(), &[0]].concat();
            let range_message = if label == "date" {
                "date out of range"
            } else {
                "time out of range"
            };
            let extra_message = if array {
                "improper binary format in array element 2"
            } else {
                "incorrect binary data format"
            };
            let cases = [
                (invalid.clone(), "22008", range_message),
                (invalid_extra, "22008", range_message),
                (
                    vec![0; width - 1],
                    "08P01",
                    "insufficient data left in message",
                ),
                (extra, "22P03", extra_message),
            ];
            for (bytes, state, message) in cases {
                let field = if array {
                    array_field(element_oid, width, &bytes)
                } else {
                    bytes
                };
                // Act
                let result = super::parse_binary_payload(
                    &copy_payload(&field),
                    &cassie,
                    &statement,
                    &controls,
                );
                // Assert
                assert!(
                    matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::TypeInput(error))) if error.sqlstate == state && error.message == message)
                );
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}
