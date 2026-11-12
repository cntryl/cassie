//! Mode String selection, ownership and cancellation witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::aggregate_exec::collection_json::{mode_string_output, OutputRow};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn plan(sql: &str) -> crate::planner::logical::LogicalPlan {
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![
            ("value".into(), crate::types::DataType::Text),
            ("keep".into(), crate::types::DataType::Boolean),
        ],
    );
    let parsed = crate::sql::parse_statement(sql).expect("parse");
    crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan")
}

fn row(value: Value, keep: bool) -> BatchRow {
    BatchRow::new(vec![
        ("value".into(), value),
        ("keep".into(), Value::Bool(keep)),
    ])
}

fn run(values: &[Value], direction: &str) -> (Value, Vec<ModeProgress>) {
    let plan = plan(&format!(
        "SELECT mode() WITHIN GROUP (ORDER BY value {direction}) FROM events"
    ));
    let specs = super::super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = values
        .iter()
        .cloned()
        .map(|value| row(value, true))
        .collect::<Vec<_>>();
    let mut events = Vec::new();
    let output = evaluate_owned_observed(&specs[0].function, &rows, &context, |progress| {
        events.push(progress);
    })
    .expect("mode");
    let value = output.into_value();
    assert_eq!(controls.current_query_memory_bytes(), 0);
    (value, events)
}

#[test]
fn should_preserve_string_ties_strict_winners_with_first_equal_representative() {
    // Arrange
    let strings = ["b", "a", "b", "a"].map(|value| Value::String(value.into()));
    let winner = ["a", "a", "b", "b", "b"].map(|value| Value::String(value.into()));
    let legacy = "2020-01-01T00:00:00Z";
    let fixed = "2020-01-01T00:00:00.000000Z";
    let equivalent = [Value::String(legacy.into()), Value::String(fixed.into())];

    // Act / Assert
    // Assert
    assert_eq!(run(&strings, "ASC").0, Value::String("a".into()));
    assert_eq!(run(&strings, "DESC").0, Value::String("b".into()));
    assert_eq!(run(&winner, "ASC").0, Value::String("b".into()));
    assert_eq!(run(&equivalent, "ASC").0, Value::String(legacy.into()));
    assert_eq!(run(&equivalent, "DESC").0, Value::String(legacy.into()));
}

#[test]
fn should_preserve_nonstring_candidate_cloning_in_mixed_winner_transitions() {
    // Arrange
    let values = [
        Value::Int64(1),
        Value::Int64(1),
        Value::String("x".into()),
        Value::String("x".into()),
        Value::String("x".into()),
    ];

    // Act
    let (value, events) = run(&values, "ASC");

    // Assert
    assert_eq!(value, Value::String("x".into()));
    let candidates = events
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                ModeProgress::OrdinaryCandidate | ModeProgress::StringCandidate
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        candidates,
        [
            ModeProgress::OrdinaryCandidate,
            ModeProgress::StringCandidate
        ]
    );
    let reverse = [
        Value::Int64(1),
        Value::Int64(1),
        Value::Int64(1),
        Value::String("x".into()),
        Value::String("x".into()),
    ];
    let (value, events) = run(&reverse, "DESC");
    assert_eq!(value, Value::Int64(1));
    let candidates = events
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                ModeProgress::OrdinaryCandidate | ModeProgress::StringCandidate
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        candidates,
        [
            ModeProgress::StringCandidate,
            ModeProgress::OrdinaryCandidate
        ]
    );
    assert_eq!(
        run(&[Value::Int64(1), Value::Float64(1.0)], "ASC").0,
        Value::Int64(1)
    );
}

#[test]
fn should_preserve_mode_null_filter_with_empty_input_behavior() {
    // Arrange
    let plan = plan("SELECT mode() WITHIN GROUP (ORDER BY value) FILTER (WHERE keep) FROM events");
    let specs = super::super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [
        row(Value::Null, true),
        row(Value::String("ignored".into()), false),
        row(Value::String("kept".into()), true),
    ];

    // Act
    let selected = evaluate_owned_observed(&specs[0].function, &rows, &context, |_| {})
        .expect("filtered")
        .into_value();
    let empty = evaluate_owned_observed(&specs[0].function, &rows[..2], &context, |_| {})
        .expect("empty")
        .into_value();

    // Assert
    assert_eq!(selected, Value::String("kept".into()));
    assert_eq!(empty, Value::Null);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_mode_string_through_aliases_with_last_row_clone() {
    // Arrange
    let plan = plan("SELECT mode() WITHIN GROUP (ORDER BY value) FROM events");
    let specs = super::super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [row(Value::String("λ-value".into()), true)];

    // Act
    let output =
        evaluate_owned_observed(&specs[0].function, &rows, &context, |_| {}).expect("mode");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("row");
    row.append(output, &["mode".into(), "alias".into()])
        .expect("aliases");
    let row = row.finish();
    let last = row.clone();
    drop(row);

    // Assert
    assert_eq!(last.get("mode"), Some(&Value::String("λ-value".into())));
    assert_eq!(last.get("alias"), Some(&Value::String("λ-value".into())));
    assert!(last.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_mode_string_result_with_source_at_exact_budget() {
    // Arrange
    let text = "λ-mode";
    let source_bytes = 64;
    for (budget, succeeds) in [
        (source_bytes + text.len(), true),
        (source_bytes + text.len() - 1, false),
    ] {
        let controls = controls(budget);
        let source = controls.reserve_query_memory(source_bytes).expect("source");

        // Act
        let result = mode_string_output(text, &source, &controls, || {});

        // Assert
        assert_eq!(result.is_ok(), succeeds);
        if let Ok(output) = result {
            assert_eq!(
                controls.current_query_memory_bytes(),
                source_bytes + text.len()
            );
            drop(output);
        }
        assert_eq!(controls.current_query_memory_bytes(), source_bytes);
        drop(source);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_a_mode_string_source_from_another_tracker() {
    // Arrange
    let first = controls(4096);
    let second = controls(4096);
    let source = first.reserve_query_memory(64).expect("source");

    // Act
    let result = mode_string_output("text", &source, &second, || {});

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(first.current_query_memory_bytes(), 64);
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(source);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_row_owner_for_empty_mode_string() {
    // Arrange
    let controls = controls(0);
    let source = controls.reserve_query_memory(0).expect("source");
    let output = mode_string_output("", &source, &controls, || {}).expect("empty result");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("empty row");

    // Act
    let result = row.append(output, &["mode".into()]);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(row);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_empty_mode_string_through_aliases() {
    // Arrange
    let controls = controls(4096);
    let source = controls.reserve_query_memory(0).expect("source");
    let output = mode_string_output("", &source, &controls, || {}).expect("empty result");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("row");

    // Act
    row.append(output, &["mode".into(), "alias".into()])
        .expect("aliases");
    let row = row.finish();

    // Assert
    assert_eq!(row.get("mode"), Some(&Value::String(String::new())));
    assert_eq!(row.get("alias"), Some(&Value::String(String::new())));
    assert!(row.query_memory().is_some());
    drop(row);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_mode_after_an_actual_equal_key_comparison() {
    // Arrange
    let plan = plan("SELECT mode() WITHIN GROUP (ORDER BY value) FROM events");
    let specs = super::super::super::aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [
        row(Value::String("x".into()), true),
        row(Value::String("x".into()), true),
    ];
    let mut seen = false;

    // Act
    let result = evaluate_owned_observed(&specs[0].function, &rows, &context, |progress| {
        if matches!(progress, ModeProgress::TieComparison) {
            seen = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(seen);
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_mode_during_actual_utf8_output_copy_with_input_owned() {
    // Arrange
    let plan = plan("SELECT mode() WITHIN GROUP (ORDER BY value) FROM events");
    let specs = super::super::super::aggregate_specs(&plan);
    let text = "λ".repeat(2048);
    let rows = [row(Value::String(text.clone()), true)];
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let mut seen = false;

    // Act
    let result = evaluate_owned_observed(&specs[0].function, &rows, &context, |progress| {
        if matches!(progress, ModeProgress::OutputChunk) {
            seen = true;
            assert!(controls.current_query_memory_bytes() >= 2 * text.len());
            cancellation.cancel();
        }
    });

    // Assert
    assert!(seen);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
