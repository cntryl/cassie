use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;

#[test]
fn should_keep_timetz_binary_receive_sqlstates_in_copy() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_timetz_bounds (value TIMETZ)",
            vec![],
        )
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_timetz_bounds (value) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let frame = |time: i64, zone: i32| {
        [time.to_be_bytes().as_slice(), zone.to_be_bytes().as_slice()].concat()
    };
    let cases = [
        (frame(-1, i32::MIN), "22008", "time out of range"),
        (
            frame(0, -57_600),
            "22009",
            "time zone displacement out of range",
        ),
        (
            frame(0, i32::MIN),
            "22009",
            "time zone displacement out of range",
        ),
        (vec![0; 11], "08P01", "insufficient data left in message"),
        (vec![0; 13], "22P03", "incorrect binary data format"),
        (
            [frame(-1, 0), vec![0]].concat(),
            "22008",
            "time out of range",
        ),
    ];
    // Act / Assert
    for (field, state, message) in cases {
        let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
        payload.extend_from_slice(&field);
        payload.extend_from_slice(&(-1_i16).to_be_bytes());
        let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
        // Assert
        assert!(
            matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::TypeInput(error))) if error.sqlstate == state && error.message == message)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_timetz_array_element_binary_errors_in_copy() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_timetz_array_bounds (value TIMETZ[])",
            vec![],
        )
        .expect("table");
    let parsed =
        crate::sql::parser::parse_statement("COPY copy_timetz_array_bounds (value) FROM STDIN")
            .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let frame = |time: i64, zone: i32| {
        [time.to_be_bytes().as_slice(), zone.to_be_bytes().as_slice()].concat()
    };
    let cases = [
        (frame(-1, 0), "22008", "time out of range"),
        (
            frame(0, -57_600),
            "22009",
            "time zone displacement out of range",
        ),
        (vec![0; 11], "08P01", "insufficient data left in message"),
        (
            vec![0; 13],
            "22P03",
            "improper binary format in array element 1",
        ),
    ];
    // Act / Assert
    for (field, state, message) in cases {
        let mut array = Vec::new();
        for value in [
            1_i32,
            0,
            1266,
            1,
            1,
            i32::try_from(field.len()).expect("length"),
        ] {
            array.extend_from_slice(&value.to_be_bytes());
        }
        array.extend_from_slice(&field);
        let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&i32::try_from(array.len()).expect("length").to_be_bytes());
        payload.extend_from_slice(&array);
        payload.extend_from_slice(&(-1_i16).to_be_bytes());
        let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
        // Assert
        assert!(
            matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::TypeInput(error))) if error.sqlstate == state && error.message == message)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_report_flattened_timetz_array_element_excess_before_later_missing_data() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_timetz_array_order (value TIMETZ[])",
            vec![],
        )
        .expect("table");
    let parsed =
        crate::sql::parser::parse_statement("COPY copy_timetz_array_order (value) FROM STDIN")
            .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let valid = vec![0_u8; 12];
    let excess = vec![0_u8; 13];
    let cases = [
        (vec![2_i32], vec![valid.clone(), excess.clone()], 2),
        (
            vec![2, 2],
            vec![valid.clone(), valid.clone(), excess.clone(), valid],
            3,
        ),
        (vec![2], vec![excess], 1),
    ];
    // Act / Assert
    for (dimensions, elements, ordinal) in cases {
        let mut array = Vec::new();
        for value in [
            i32::try_from(dimensions.len()).expect("dimensions"),
            0,
            1266,
        ] {
            array.extend_from_slice(&value.to_be_bytes());
        }
        for dimension in dimensions {
            array.extend_from_slice(&dimension.to_be_bytes());
            array.extend_from_slice(&1_i32.to_be_bytes());
        }
        for element in elements {
            array.extend_from_slice(&i32::try_from(element.len()).expect("length").to_be_bytes());
            array.extend_from_slice(&element);
        }
        let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&i32::try_from(array.len()).expect("length").to_be_bytes());
        payload.extend_from_slice(&array);
        payload.extend_from_slice(&(-1_i16).to_be_bytes());
        let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
        // Assert
        assert!(
            matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::TypeInput(error)))
            if error.sqlstate == "22P03" && error.message == format!("improper binary format in array element {ordinal}"))
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
