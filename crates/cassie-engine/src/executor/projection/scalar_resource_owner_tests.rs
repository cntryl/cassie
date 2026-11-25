use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_retain_numeric_power_function_workspace_until_the_last_projected_row_drops() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let sql = "SELECT power(2::numeric, 8::numeric)";
    let session = CassieSession::new("root".into(), None)
        .with_statement_text(sql, &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(sql).expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };

    // Act
    let rows = project_rows(
        vec![BatchRow::from_projected_values(Vec::new())],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");
    let last = rows[0].clone();
    drop(rows);

    // Assert
    assert!(matches!(
        &last.entries()[0].1,
        Value::Typed(crate::types::typed::TypedValue::Numeric(_))
    ));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_numeric_power_operator_workspace_until_the_projected_row_drops() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let sql = "SELECT 2::numeric ^ 8::numeric";
    let session = CassieSession::new("root".into(), None)
        .with_statement_text(sql, &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(sql).expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };

    // Act
    let rows = project_rows(
        vec![BatchRow::from_projected_values(Vec::new())],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");

    // Assert
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_temporal_to_char_workspace_until_the_last_projected_row_drops() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let sql = "SELECT to_char(ts, 'YYYY') FROM records";
    let session = CassieSession::new("root".into(), None)
        .with_statement_text(sql, &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(sql).expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let source = BatchRow::new(vec![(
        "ts".into(),
        Value::Typed(crate::types::typed::TypedValue::Timestamp(0)),
    )]);

    // Act
    let rows = project_rows(
        vec![source],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");
    let last = rows[0].clone();
    drop(rows);

    // Assert
    assert_eq!(last.entries()[0].1, Value::String("2000".into()));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_temporal_parse_memory_when_projection_completes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let sql =
        "SELECT to_date('2000-01-02', 'YYYY-MM-DD'), to_timestamp('2000-01-02', 'YYYY-MM-DD')";
    let session = CassieSession::new("root".into(), None)
        .with_statement_text(sql, &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(sql).expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };

    // Act
    let rows = project_rows(
        vec![BatchRow::from_projected_values(Vec::new())],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");

    // Assert
    assert!(matches!(
        &rows[0].entries()[0].1,
        Value::Typed(crate::types::typed::TypedValue::Date(_))
    ));
    assert!(matches!(
        &rows[0].entries()[1].1,
        Value::Typed(crate::types::typed::TypedValue::TimestampTz(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_numeric_power_projection_when_its_result_exceeds_the_query_budget() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 8 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let sql = "SELECT power(2::numeric, 100000::numeric)";
    let session = CassieSession::new("root".into(), None)
        .with_statement_text(sql, &controls)
        .expect("session");
    let parsed = crate::sql::parse_statement(sql).expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };

    // Act
    let result = project_rows(
        vec![BatchRow::from_projected_values(Vec::new())],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
