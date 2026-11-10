use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn payload(field: &[u8]) -> Vec<u8> {
    let mut bytes = b"PGCOPY\n\xff\r\n\0".to_vec();
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&1_i16.to_be_bytes());
    bytes.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
    bytes.extend_from_slice(field);
    bytes.extend_from_slice(&(-1_i16).to_be_bytes());
    bytes
}

#[test]
fn should_parse_binary_json_as_ordinary_json_with_preserved_error_mapping() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_json_owner (value JSON)",
            vec![],
        )
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_json_owner (value) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act / Assert
    for field in [
        br#"{"b":1,"a":2,"b":3}"#.as_slice(),
        br#"["\u0061",1.2300e+5,null]"#,
        b"null",
        b"-0",
        br#"{"$serde_json::private::Number":"1E2"}"#,
        br#"{"$serde_json::private::Num\u0062er":"-0"}"#,
        br#"{"$serde_json::private::RawValue":"{\"x\":[1,null]}"}"#,
        br#"{"$serde_json::private::Number":"-xY","other":1}"#,
        br#"{"$serde_json::private::RawValue":"[1,]","other":1}"#,
        br#"{"$serde_json::private::RawValue":null}"#,
        r#"{"é":1,"e\u0301":2,"":3,"🙂":4}"#.as_bytes(),
    ] {
        let expected = crate::runtime::accounted::json_arena::parse_value(field, &controls)
            .expect("ordinary JSON")
            .get()
            .to_string();
        let (rows, memory) =
            super::parse_binary_payload(&payload(field), &cassie, &statement, &controls)
                .expect("parse");
        // Assert
        assert_eq!(rows, vec![vec![Some(expected)]]);
        drop((rows, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for field in [b"\x7bbad".as_slice(), b"true trailing", b"{bad\xff}"] {
        let result = super::parse_binary_payload(&payload(field), &cassie, &statement, &controls);
        assert!(
            matches!(result, Err(super::super::filter::CopyWhereError::Cassie(CassieError::Parse(message))) if message == "COPY binary field 1 is invalid: invalid binary json value")
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    let mut sql_null = payload(&[]);
    let field_length = b"PGCOPY\n\xff\r\n\0".len() + 8 + 2;
    sql_null[field_length..field_length + 4].copy_from_slice(&(-1_i32).to_be_bytes());
    let (rows, memory) =
        super::parse_binary_payload(&sql_null, &cassie, &statement, &controls).expect("SQL NULL");
    assert_eq!(rows, vec![vec![None]]);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_binary_json_output_owned_after_input_owner_retires() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_json_transfer (value JSON)",
            vec![],
        )
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_json_transfer (value) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let field = br#"{"long":"a\\nb","number":1.2300e+5,"null":null}"#;
    let bytes = payload(field);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let (rows, memory) =
        super::parse_binary_payload(&bytes, &cassie, &statement, &controls).expect("parse");
    let peak = controls.peak_query_memory_bytes();
    let retained = controls.current_query_memory_bytes();
    // Assert
    let expected = serde_json::from_slice::<serde_json::Value>(field)
        .expect("original")
        .to_string();
    assert_eq!(rows, vec![vec![Some(expected)]]);
    assert!(retained > 0);
    assert!(
        peak > retained,
        "input owner should retire while output remains"
    );
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: peak,
        ..CassieRuntimeLimits::default()
    };
    let exact = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (rows, memory) =
        super::parse_binary_payload(&bytes, &cassie, &statement, &exact).expect("exact peak");
    drop((rows, memory));
    assert_eq!(exact.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: peak - 1,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    assert!(super::parse_binary_payload(&bytes, &cassie, &statement, &denied).is_err());
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_binary_json_array_on_its_existing_array_codec_route() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_json_array_route (value JSON[])",
            vec![],
        )
        .expect("table");
    let parsed =
        crate::sql::parser::parse_statement("COPY copy_json_array_route (value) FROM STDIN")
            .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let field = br#"{"b":2,"a":1}"#;
    let mut array = Vec::new();
    for value in [
        1_i32,
        0,
        114,
        1,
        1,
        i32::try_from(field.len()).expect("length"),
    ] {
        array.extend_from_slice(&value.to_be_bytes());
    }
    array.extend_from_slice(field);
    let original = super::super::super::codecs::binary_to_value(&array, 199).expect("array codec");
    let expected = super::super::super::codecs::value_to_text(&original, 199);

    // Act
    let (rows, memory) =
        super::parse_binary_payload(&payload(&array), &cassie, &statement, &controls)
            .expect("copy");

    // Assert
    assert_eq!(rows, vec![vec![Some(expected)]]);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
