use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::QueryCancellationHandle;
use crate::types::extended::{numeric, ExtendedType};
use crate::types::typed::TypedValue;
use crate::types::DataType;
use std::time::Instant;

fn row() -> BatchRow {
    let text = format!("12345{}", "0".repeat(256));
    let value = Value::Typed(TypedValue::Numeric(
        numeric::parse_text(&text).expect("numeric"),
    ));
    let mut row = BatchRow::new(vec![("n".into(), value)]);
    row.set_data_types(vec![DataType::Extended(ExtendedType::Numeric(None))].into());
    row
}

fn controls(budget: usize) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(
        &limits,
        Instant::now(),
        QueryCancellationHandle::new(),
    )
}

#[test]
fn should_retain_numeric_sort_key_at_measured_budget() {
    // Arrange
    let order = [OrderExpr {
        expr: Expr::Column("n".into()),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let baseline = controls(8_000_000);
    // Act
    let key = eval
        .row_key_with_context(
            &row(),
            &order,
            &SortRetentionContext::default(),
            Some(&baseline),
        )
        .expect("numeric key");
    let peak = baseline.peak_query_memory_bytes();
    drop(key);
    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = eval.row_key_with_context(
            &row(),
            &order,
            &SortRetentionContext::default(),
            Some(&controls),
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_report_search_lineage_error_before_numeric_key_building() {
    // Arrange
    let mut row = row();
    row.append_value(
        "\0cassie_cte_lineage:walk".into(),
        Value::String("{invalid".into()),
    );
    let order = [OrderExpr {
        expr: Expr::Column("n".into()),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let controls = controls(8_000_000);
    // Act
    let result = eval.row_key_with_context(
        &row,
        &order,
        &SortRetentionContext::default(),
        Some(&controls),
    );
    // Assert
    assert!(
        matches!(result, Err(ref error) if error.to_string().contains("invalid private SEARCH lineage"))
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
