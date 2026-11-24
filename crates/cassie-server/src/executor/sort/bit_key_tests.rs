use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::QueryCancellationHandle;
use crate::types::extended::{bit_string::BitString, ExtendedType};
use crate::types::typed::TypedValue;
use crate::types::DataType;
use std::time::Instant;

fn row() -> BatchRow {
    let value = Value::Typed(TypedValue::Bit(BitString::from_bits(std::iter::repeat_n(
        true, 65_536,
    ))));
    let mut row = BatchRow::new(vec![("b".into(), value)]);
    row.set_data_types(vec![DataType::Extended(ExtendedType::Bit { length: None })].into());
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
fn should_retain_bit_sort_key_at_measured_budget() {
    // Arrange
    let order = [OrderExpr {
        expr: Expr::Column("b".into()),
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
        .expect("bit key");
    let peak = baseline.peak_query_memory_bytes();
    // Assert
    assert!(baseline.current_query_memory_bytes() >= 8192);
    drop(key);
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
fn should_preserve_search_lineage_error_before_bit_key_building() {
    // Arrange
    let mut row = row();
    row.append_value(
        "\0cassie_cte_lineage:walk".into(),
        Value::String("{invalid".into()),
    );
    let order = [OrderExpr {
        expr: Expr::Column("b".into()),
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
