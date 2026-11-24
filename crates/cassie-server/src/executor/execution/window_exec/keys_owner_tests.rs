use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn call() -> WindowFunctionCall {
    let parsed =
        crate::sql::parse_statement("SELECT row_number() OVER (ORDER BY n)").expect("window SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let SelectItem::WindowFunction { function, .. } =
        select.projection.into_iter().next().expect("projection")
    else {
        panic!("window");
    };
    function
}

fn controls(budget: usize, cancel: QueryCancellationHandle) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancel)
}

#[test]
fn should_keep_peer_with_sort_key_reservations_until_drop() {
    // Arrange
    let function = call();
    let row = BatchRow::new(vec![("n".into(), Value::String("payload".repeat(64)))]);
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    // Act
    let peer = window_peer_key(&row, &function.order_by, &context).expect("peer");
    let order = window_sort_key(&row, &function.order_by, &context).expect("sort key");
    // Assert
    assert!(controls.current_query_memory_bytes() > 0);
    drop(peer);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(order);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_equal_partition_order_with_release_every_reservation() {
    // Arrange
    let function = call();
    let rows = vec![BatchRow::new(vec![("n".into(), Value::Int64(1))]); 3];
    let mut indices = [2, 0, 1];
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    // Act
    sort_partition_observing(&rows, &function, &mut indices, &context, |_| {}).expect("stable");
    // Assert
    assert_eq!(indices, [2, 0, 1]);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_an_actual_key_without_modifying_indices() {
    // Arrange
    let function = call();
    let rows = vec![
        BatchRow::new(vec![("n".into(), Value::Int64(2))]),
        BatchRow::new(vec![("n".into(), Value::Int64(1))]),
    ];
    let mut indices = [0, 1];
    let cancel = QueryCancellationHandle::new();
    let controls = controls(1_000_000, cancel.clone());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    let mut built = 0;
    // Act
    let result = sort_partition_observing(&rows, &function, &mut indices, &context, |_| {
        built += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(built, 1);
    assert!(result.is_err());
    assert_eq!(indices, [0, 1]);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_window_key_peak_budget() {
    // Arrange
    let function = call();
    let row = BatchRow::new(vec![("n".into(), Value::String("payload".repeat(64)))]);
    let baseline = controls(1_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &baseline,
        evaluate: None,
        output_memory: None,
    };
    // Act
    let result = window_sort_key(&row, &function.order_by, &context).expect("measure");
    let peak = baseline.peak_query_memory_bytes();
    drop(result);
    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget, QueryCancellationHandle::new());
        let context = WindowExecutionContext {
            controls: &controls,
            ..context
        };
        let result = window_sort_key(&row, &function.order_by, &context);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_key_after_its_source_row_owner_drops() {
    // Arrange
    let function = call();
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let source = std::sync::Arc::new(controls.reserve_query_memory(256).expect("source"));
    let weak = std::sync::Arc::downgrade(&source);
    let row = BatchRow::new(vec![("n".into(), Value::String("payload".repeat(64)))])
        .with_query_memory(Some(source));
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    // Act
    let key = window_sort_key(&row, &function.order_by, &context).expect("key");
    drop(row);
    // Assert
    assert!(weak.upgrade().is_none());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(key);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_integral_numeric_with_integer_peer_equality() {
    // Arrange
    let function = call();
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    let numeric = crate::types::extended::numeric::parse_text("1.00").expect("numeric");
    let mut typed = BatchRow::new(vec![(
        "n".into(),
        Value::Typed(crate::types::typed::TypedValue::Numeric(numeric)),
    )]);
    typed.set_data_types(
        vec![crate::types::DataType::Extended(
            crate::types::extended::ExtendedType::Numeric(None),
        )]
        .into(),
    );
    let integer = BatchRow::new(vec![("n".into(), Value::Int64(1))]);
    // Act
    let left = window_peer_key(&typed, &function.order_by, &context).expect("numeric peer");
    let right = window_peer_key(&integer, &function.order_by, &context).expect("integer peer");
    // Assert
    assert!(left == right);
    drop(left);
    drop(right);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_jsonb_fallback_peer_equality_with_sorting() {
    // Arrange
    let function = call();
    let controls = controls(1_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    let mut rows = vec![
        BatchRow::new(vec![(
            "n".into(),
            Value::Json(serde_json::json!({"a": [1, null]})),
        )]),
        BatchRow::new(vec![(
            "n".into(),
            Value::Json(serde_json::json!({"a": [1, null]})),
        )]),
    ];
    for row in &mut rows {
        row.set_data_types(vec![crate::types::DataType::Jsonb].into());
    }
    let mut indices = [1, 0];
    // Act
    let left = window_peer_key(&rows[0], &function.order_by, &context).expect("first peer");
    let right = window_peer_key(&rows[1], &function.order_by, &context).expect("second peer");
    sort_partition_observing(&rows, &function, &mut indices, &context, |_| {})
        .expect("JSONB fallback sort");
    // Assert
    assert!(left == right);
    assert_eq!(indices, [1, 0]);
    drop(left);
    drop(right);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_mixed_vector_array_on_the_legacy_window_key_path() {
    // Arrange
    let function = call();
    let controls = controls(2_000_000, QueryCancellationHandle::new());
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        evaluate: None,
        output_memory: None,
    };
    let array = crate::types::SqlArray::from_elements(vec![
        Value::Vector(crate::types::Vector::new(vec![1.0; 4096])),
        Value::Int64(7),
    ])
    .expect("public mixed array");
    let value = Value::Array(array);
    let mut row = BatchRow::new(vec![("n".into(), value.clone())]);
    row.set_data_types(
        vec![crate::types::DataType::Array(Box::new(
            crate::types::DataType::Vector(Some(4096)),
        ))]
        .into(),
    );
    let expected = crate::executor::array_order::key(
        &row,
        &function.order_by[0].expr,
        &value,
        &functions,
        Some(&controls),
    )
    .expect("legacy key");
    let verification_controls = self::controls(2_000_000, QueryCancellationHandle::new());
    let verification_context = WindowExecutionContext {
        controls: &verification_controls,
        ..context
    };
    let sort_key = window_sort_key(&row, &function.order_by, &verification_context)
        .expect("fallback sort key");
    assert_eq!(sort_key.parts[0].value, expected);
    drop(sort_key);
    assert_eq!(verification_controls.current_query_memory_bytes(), 0);
    // Act
    let actual = window_peer_key(&row, &function.order_by, &context).expect("fallback key");
    // Assert
    let peak = controls.peak_query_memory_bytes();
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limited = self::controls(budget, QueryCancellationHandle::new());
        let limited_context = WindowExecutionContext {
            controls: &limited,
            ..context
        };
        let result = window_peer_key(&row, &function.order_by, &limited_context);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}
