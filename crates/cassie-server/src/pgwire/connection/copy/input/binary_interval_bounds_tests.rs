use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

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

fn array_field(value: &[u8]) -> Vec<u8> {
    let mut array = Vec::new();
    for number in [1_i32, 0, 1186, 2, 1, 16] {
        array.extend_from_slice(&number.to_be_bytes());
    }
    array.extend_from_slice(&[0; 16]);
    array.extend_from_slice(&i32::try_from(value.len()).expect("length").to_be_bytes());
    array.extend_from_slice(value);
    array
}

#[test]
fn should_preserve_interval_binary_errors_in_scalar_with_array_copy() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    for array in [false, true] {
        let suffix = if array { "[]" } else { "" };
        let name = if array {
            "interval_array_bounds"
        } else {
            "interval_scalar_bounds"
        };
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {name} (value INTERVAL{suffix})"),
                vec![],
            )
            .expect("table");
        let parsed =
            crate::sql::parser::parse_statement(&format!("COPY {name} (value) FROM STDIN"))
                .expect("parse");
        let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
            panic!("COPY")
        };
        let extra_message = if array {
            "improper binary format in array element 2"
        } else {
            "incorrect binary data format"
        };
        for (bytes, state, message) in [
            (vec![0; 15], "08P01", "insufficient data left in message"),
            (vec![0; 17], "22P03", extra_message),
        ] {
            let field = if array { array_field(&bytes) } else { bytes };
            // Act
            let result =
                super::parse_binary_payload(&copy_payload(&field), &cassie, &statement, &controls);
            // Assert
            assert!(
                matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::TypeInput(error))) if error.sqlstate == state && error.message == message)
            );
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
