use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

#[test]
fn should_admit_rebuilt_rich_window_lookup_after_large_prior_alias() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 70_000 + 8192,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let parent = Arc::new(
        controls
            .reserve_query_memory(70_000)
            .expect("admitted original body"),
    );
    let operator = Arc::new(controls.reserve_query_memory(64).expect("prior operator"));
    let mut row = BatchRow::new(vec![("n".into(), Value::Json(serde_json::json!([1])))])
        .with_optional_data_types(Some(Arc::new(vec![
            DataType::Array(Box::new(DataType::Json)),
            DataType::BigInt,
        ])))
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(&controls, operator)
        .expect("prior owner chain");
    row.append_value("a".repeat(65_536), Value::Int64(1));
    let function = WindowFunctionCall {
        name: "rank".into(),
        args: Vec::new(),
        partition_by: Vec::new(),
        order_by: vec![crate::sql::ast::OrderExpr {
            expr: Expr::Column("n".into()),
            direction: SortDirection::Asc,
            nulls: None,
        }],
        frame: None,
    };
    let selection = Selection {
        partition: Vec::new(),
        order: vec![0],
        payload: None,
    };
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
    };
    // Act
    let result = keys(&[row], &function, &selection, &context);
    // Assert
    assert!(
        matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ),
        "a rebuilt lookup must admit its copied long prior name"
    );
    assert_eq!(controls.current_query_memory_bytes(), 70_000);
    drop(parent);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
