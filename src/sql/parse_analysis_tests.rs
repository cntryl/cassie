//! Resource and raw-tree ownership at semantic Parse analysis.
use super::parse_analysis::validate;
use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::{binder::BindingContext, parse_statement};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_reject_analysis_copy_given_small_budget_when_literal_payload_is_large() {
    // Arrange
    let parsed =
        parse_statement(&format!("SELECT '{}'::TEXT", "x".repeat(131_072))).expect("parse");
    let controls = controls(1024);
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_analysis_copy_given_valid_query_when_binding_succeeds() {
    // Arrange
    let parsed = parse_statement("SELECT v.n FROM (VALUES (7::INT)) v(n)").expect("parse");
    let before = serde_json::to_value(&parsed).expect("raw tree");
    let controls = controls(1_048_576);
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(serde_json::to_value(&parsed).expect("raw tree"), before);
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_analysis_copy_given_invalid_input_when_binding_fails() {
    // Arrange
    let parsed =
        parse_statement("SELECT CASE WHEN FALSE THEN 'bad'::INT ELSE 0 END").expect("parse");
    let controls = controls(1_048_576);
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::Execution(_))));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_analysis_given_cancelled_request_when_no_copy_has_been_allocated() {
    // Arrange
    let parsed = parse_statement("SELECT 7").expect("parse");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_expire_analysis_given_elapsed_deadline_when_no_copy_has_been_allocated() {
    // Arrange
    let parsed = parse_statement("SELECT 7").expect("parse");
    let mut controls = controls(1_048_576);
    controls.deadline = Some(Instant::now());
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_analysis_given_excessive_nesting_when_borrowed_preflight_visits_children() {
    // Arrange
    let mut parsed = parse_statement("SELECT 7").expect("parse");
    for _ in 0..130 {
        parsed = crate::sql::ast::ParsedStatement {
            raw_sql: String::new(),
            statement: crate::sql::ast::QueryStatement::Explain(
                crate::sql::ast::ExplainStatement {
                    analyze: false,
                    statement: Box::new(parsed),
                },
            ),
        };
    }
    let controls = controls(1_048_576);
    // Act
    let result = validate(
        &parsed,
        &[],
        &Catalog::new(),
        &BindingContext::default(),
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_reject_nested_clone_payloads_given_short_raw_sql_when_preflight_measures_owned_nodes() {
    // Arrange
    let payload = "x".repeat(131_072);
    let queries = [
        format!("SELECT lower('{payload}')"),
        format!("SELECT ROW('{payload}', 1) = ROW('x', 1)"),
        format!("SELECT CASE WHEN TRUE THEN '{payload}' ELSE '' END"),
        format!("SELECT 1 AS \"{payload}\""),
        format!("SELECT 1 FROM \"{payload}\""),
        format!("SELECT row_number() OVER (ORDER BY '{payload}')"),
        format!("SELECT 1 FROM (VALUES (1)) v(\"{payload}\")"),
    ];
    // Act
    let outcomes = queries
        .iter()
        .map(|sql| {
            let mut parsed = parse_statement(sql).expect("parse payload fixture");
            parsed.raw_sql.clear();
            let controls = controls(16_384);
            let result = validate(
                &parsed,
                &[],
                &Catalog::new(),
                &BindingContext::default(),
                &controls,
            );
            (
                result,
                controls.peak_query_memory_bytes(),
                controls.current_query_memory_bytes(),
            )
        })
        .collect::<Vec<_>>();
    // Assert
    for (result, peak, current) in outcomes {
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{result:?}"
        );
        assert_eq!(peak, 0);
        assert_eq!(current, 0);
    }
}
