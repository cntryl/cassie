use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_retain_owned_object_keys_through_projection_expansion_and_last_row() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::String(r#"{"z":1,"a":2}"#.into()),
    )]);
    let parsed =
        crate::sql::parse_statement("SELECT json_object_keys(doc) FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    let mut functions = std::collections::HashMap::new();
    functions.insert(
        "json_object_keys".into(),
        crate::catalog::FunctionMeta {
            name: "json_object_keys".into(),
            args: vec![],
            return_type: crate::types::DataType::Text,
            volatility: crate::catalog::Volatility::Immutable,
            body: "'not the builtin'".into(),
        },
    );
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
    assert!(controls.current_query_memory_bytes() > baseline);
    let expanded =
        json_object_keys::expand(vec![rows], &select.projection, &controls).expect("expansion");
    let values: Vec<_> = expanded
        .iter()
        .flatten()
        .map(|row| row.entries()[0].1.clone())
        .collect();
    let last = expanded[0][0].clone();
    drop(expanded);
    // Assert
    assert_eq!(
        values,
        vec![Value::String("a".into()), Value::String("z".into())]
    );
    assert!(controls.current_query_memory_bytes() > baseline);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_object_key_projection_when_cancelled_before_row_attachment() {
    // Arrange
    let cancel = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = BatchRow::new(vec![("doc".into(), Value::String(r#"{"a":1}"#.into()))]);
    let parsed =
        crate::sql::parse_statement("SELECT jsonb_object_keys(doc) FROM records").expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    character_casts::cancel_before_attach_once(cancel.clone());
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
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_select_only_the_existing_single_bare_object_keys_projection() {
    // Arrange
    let cases = [
        ("SELECT json_object_keys(doc) FROM records", true),
        ("SELECT JSONB_OBJECT_KEYS(doc) FROM records", true),
        ("SELECT json_object_keys(doc), 1 FROM records", false),
        (
            "SELECT pg_catalog.json_object_keys(doc) FROM records",
            false,
        ),
        ("SELECT json_typeof(doc) FROM records", false),
    ];
    // Act / Assert
    for (sql, expected) in cases {
        let parsed = crate::sql::parse_statement(sql).expect("SQL");
        let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
            panic!("SELECT")
        };
        assert_eq!(
            json_object_keys::requires_owner(&select.projection),
            expected,
            "{sql}"
        );
    }
}
