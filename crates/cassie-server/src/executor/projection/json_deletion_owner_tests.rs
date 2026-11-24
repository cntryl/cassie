use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn typed_json_row(value: serde_json::Value) -> BatchRow {
    let mut row = BatchRow::new(vec![("doc".into(), Value::Json(value))]);
    row.set_data_types(vec![crate::types::DataType::Jsonb].into());
    row
}

#[test]
fn should_retain_session_controlled_deletion_output_on_the_projected_row() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":"payload"}));
    let parsed = crate::sql::parse_statement("SELECT doc - 'drop' FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let functions = std::collections::HashMap::new();
    // Act
    let rows = project_rows(
        vec![row],
        &select.projection,
        &[],
        None,
        &functions,
        Some(&session),
    )
    .expect("projection");
    // Assert
    assert_eq!(rows.len(), 1);
    assert!(
        matches!(&rows[0].entries()[0].1, Value::String(text) if text == r#"{"keep": "payload"}"#)
    );
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_json_builder_result_through_the_last_projected_clone() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed =
        crate::sql::parse_statement("SELECT json_build_object('a', 1) ->> 'a' FROM records")
            .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![BatchRow::new(vec![])],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == "1"));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_mutation_result_through_the_last_projected_clone() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(
        "SELECT jsonb_set('{\"a\":1}'::JSONB, '{a}', '2'::JSONB) ->> 'a' FROM records",
    )
    .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![BatchRow::new(vec![])],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == "2"));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_path_function_result_through_the_last_projected_clone() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(
        "SELECT jsonb_extract_path('{\"a\":{\"b\":1}}'::JSONB, 'a') ->> 'b' FROM records",
    )
    .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![BatchRow::new(vec![])],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == "1"));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_jsonpath_tree_workspace_through_the_last_projected_clone() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let parsed = crate::sql::parse_statement(
        "SELECT jsonb_path_query_array('{\"a\":[\"x\"]}'::JSONB, '$.a[*]') ->> 0 FROM records",
    )
    .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![BatchRow::new(vec![])],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == "x"));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_json_concat_until_last_projection_clone_without_changing_siblings() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let mut row = BatchRow::new(vec![
        ("doc".into(), Value::Json(serde_json::json!({"a":1}))),
        ("n".into(), Value::Int64(2)),
    ]);
    row.set_data_types(
        vec![
            crate::types::DataType::Jsonb,
            crate::types::DataType::BigInt,
        ]
        .into(),
    );
    let parsed = crate::sql::parse_statement(
        "SELECT doc || doc AS j, 'a' || 'b' AS s, n - 1 AS n FROM records",
    )
    .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![row],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == r#"{"a": 1}"#));
    assert!(matches!(&last.entries()[1].1, Value::String(text) if text == "ab"));
    assert!(matches!(&last.entries()[2].1, Value::Int64(1)));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_session_controlled_extraction_until_last_row_clone() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = typed_json_row(serde_json::json!({"a":"payload"}));
    let parsed = crate::sql::parse_statement("SELECT doc ->> 'a' FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![row],
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
    assert!(matches!(&last.entries()[0].1, Value::String(text) if text == "payload"));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_dynamic_json_subtraction_on_an_untyped_column() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"drop":1,"keep":2})),
    )]);
    let parsed = crate::sql::parse_statement("SELECT doc - 'drop' FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![row],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");
    // Assert
    assert!(matches!(&rows[0].entries()[0].1, Value::String(text) if text == r#"{"keep": 2}"#));
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_duplicate_deletion_projections_until_the_last_row_clone_drops() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":"payload"}));
    let parsed =
        crate::sql::parse_statement("SELECT doc - 'drop' AS a, doc - 'drop' AS b FROM records")
            .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![row],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");
    let clone = rows[0].clone();
    drop(rows);
    // Assert
    assert_eq!(clone.entries().len(), 2);
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(clone);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_numeric_with_temporal_subtraction_outside_deletion_owner_detection() {
    // Arrange
    let row = BatchRow::new(vec![("n".into(), Value::Int64(2))]);
    let functions = std::collections::HashMap::new();
    let parsed = crate::sql::parse_statement("SELECT n - 1, '2020-01-02'::DATE - 1 FROM records")
        .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let ops = compile_projection_ops(&select.projection);
    // Act / Assert
    // Assert
    assert!(!character_casts::deletion_required(
        &ops,
        &row,
        &[],
        &functions
    ));
}

#[test]
fn should_preserve_numeric_sibling_result_when_projecting_json_deletion() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let mut row = BatchRow::new(vec![
        ("n".into(), Value::Int64(2)),
        (
            "doc".into(),
            Value::Json(serde_json::json!({"drop":1,"keep":2})),
        ),
    ]);
    row.set_data_types(
        vec![
            crate::types::DataType::BigInt,
            crate::types::DataType::Jsonb,
        ]
        .into(),
    );
    let parsed = crate::sql::parse_statement("SELECT n - 1 AS n, doc - 'drop' AS j FROM records")
        .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    // Act
    let rows = project_rows(
        vec![row],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    )
    .expect("projection");
    // Assert
    assert!(matches!(&rows[0].entries()[0].1, Value::Int64(1)));
    assert!(matches!(&rows[0].entries()[1].1, Value::String(text) if text == r#"{"keep": 2}"#));
}

#[test]
fn should_release_a_produced_deletion_row_when_canceled_before_owner_attachment() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = typed_json_row(serde_json::json!({"drop":1,"keep":"payload"}));
    let parsed = crate::sql::parse_statement("SELECT doc - 'drop' FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    character_casts::cancel_before_attach_once(cancellation.clone());
    // Act
    let result = project_rows(
        vec![row],
        &select.projection,
        &[],
        None,
        &std::collections::HashMap::new(),
        Some(&session),
    );
    // Assert
    assert!(cancellation.is_canceled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
