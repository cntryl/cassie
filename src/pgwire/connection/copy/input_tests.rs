use super::parse_text_payload;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_preserve_text_copy_fields_while_holding_decoder_admission() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\N\ta\\tb\r\n\\\\N\tlast";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![
            vec![None, Some("a\tb".into())],
            vec![Some("\\N".into()), Some("last".into())]
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_text_copy_row_directory_before_allocating_over_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = parse_text_payload(b"one\ttwo\n", '\t', "\\N", &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_text_copy_escape_and_final_record_behavior() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\101\t\\x41\tend\\";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Some("101".into()),
            Some("x41".into()),
            Some("end\\".into())
        ]]
    );
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_binary_row_directory_admission_through_output() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_binary_budget (id INT)", vec![])
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_binary_budget (id) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&4_i32.to_be_bytes());
    payload.extend_from_slice(&42_i32.to_be_bytes());
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let (rows, memory) =
        super::parse_binary_payload(&payload, &cassie, &statement, &controls).expect("binary");
    // Assert
    assert_eq!(rows, vec![vec![Some("42".into())]]);
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_binary_row_directory_when_budget_is_exhausted() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_binary_small (id INT)", vec![])
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_binary_small (id) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&(-1_i32).to_be_bytes());
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: std::mem::size_of::<i64>(),
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = super::parse_binary_payload(&payload, &cassie, &statement, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_stop_binary_column_lookup_when_copy_is_cancelled() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let parsed = crate::sql::parser::parse_statement("COPY t (id) FROM STDIN").expect("COPY");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    cancellation.cancel();
    // Act
    let result = super::copy_column_types(&statement, &[], &controls);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
