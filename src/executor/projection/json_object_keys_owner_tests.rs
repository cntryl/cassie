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

#[test]
fn should_retain_value_backed_object_keys_through_single_projection_expansion() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let session = CassieSession::new("root".into(), None)
        .with_statement_text("SELECT 1", &controls)
        .expect("session");
    let baseline = controls.current_query_memory_bytes();
    let row = BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"z":1,"a":2})),
    )]);
    let parsed =
        crate::sql::parse_statement("SELECT jsonb_object_keys(doc) FROM records").expect("SQL");
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
    assert!(controls.current_query_memory_bytes() > baseline);
    let expanded =
        json_object_keys::expand(vec![rows], &select.projection, &controls).expect("expansion");
    let values: Vec<_> = expanded
        .iter()
        .flatten()
        .map(|row| row.entries()[0].1.clone())
        .collect();
    // Assert
    assert_eq!(
        values,
        vec![Value::String("a".into()), Value::String("z".into())]
    );
    drop(expanded);
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn expansion_fixture(keys: serde_json::Value) -> (Vec<Batch>, Vec<crate::sql::ast::SelectItem>) {
    let parsed = crate::sql::parse_statement("SELECT json_object_keys(doc) AS keys FROM records")
        .expect("SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    (
        vec![vec![BatchRow::new(vec![(
            "keys".into(),
            Value::Json(keys),
        )])]],
        select.projection,
    )
}

#[test]
fn should_release_prepared_object_key_rows_after_actual_copy_directory_and_slot_cancellation() {
    // Arrange
    for stage in 0..4 {
        let cancel = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let key = format!("{}é-end", "x".repeat(1_023));
        let (batches, projection) = expansion_fixture(serde_json::json!([key]));
        json_object_keys::cancel_expand_once(cancel.clone(), stage);
        // Act
        let result = json_object_keys::expand(batches, &projection, &controls);
        // Assert
        assert!(cancel.is_cancelled(), "stage {stage}");
        assert!(result.is_err(), "stage {stage}");
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_expanded_alias_unicode_order_and_last_row_owner() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let first = format!("{}é-end", "x".repeat(1_023));
    let (batches, projection) = expansion_fixture(serde_json::json!([first.clone(), "last"]));
    // Act
    let expanded = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    let keys: Vec<_> = expanded
        .iter()
        .flatten()
        .map(|row| row.entries()[0].clone())
        .collect();
    let last = expanded[0][0].clone();
    drop(expanded);
    // Assert
    assert_eq!(keys[0], ("keys".into(), Value::String(first)));
    assert_eq!(keys[1], ("keys".into(), Value::String("last".into())));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_null_and_nontext_error_order_while_releasing_partial_expansion() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let (mut batches, projection) = expansion_fixture(serde_json::json!(["first", 2]));
    batches[0].insert(0, BatchRow::new(vec![("keys".into(), Value::Null)]));
    // Act
    let result = json_object_keys::expand(batches, &projection, &controls);
    // Assert
    assert!(result
        .expect_err("non-text")
        .to_string()
        .contains("non-text key"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_prepared_expansion_peak() {
    // Arrange
    let make = || expansion_fixture(serde_json::json!(["first", "second"]));
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let (batches, projection) = make();
    let result = json_object_keys::expand(batches, &projection, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let (batches, projection) = make();
        let result = json_object_keys::expand(batches, &projection, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_source_lease_until_independent_expanded_rows_exist() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let source = std::sync::Arc::new(controls.reserve_query_memory(64).expect("source lease"));
    let source_weak = std::sync::Arc::downgrade(&source);
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    json_object_keys::observe_source_during_copy(
        source_weak.clone(),
        std::rc::Rc::clone(&observed),
    );
    let (mut batches, projection) = expansion_fixture(serde_json::json!(["first", "second"]));
    batches[0][0] = batches[0][0].clone().with_query_memory(Some(source));
    // Act
    let result = json_object_keys::expand(batches, &projection, &controls).expect("expansion");
    // Assert
    assert!(observed.get());
    assert!(source_weak.upgrade().is_none());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
