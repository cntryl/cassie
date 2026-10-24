use std::collections::HashMap;
use std::time::Instant;

use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::executor::execution::build_logical_plan;

#[path = "control_tests.rs"]
mod controls;

#[path = "modifier_tests.rs"]
mod modifiers;

const SERIAL_ROW_COUNT: usize = 50_000;
const SERIAL_GROUP_COUNT: usize = 4;

fn tenant_row(index: usize) -> BatchRow {
    BatchRow::new(vec![
        (
            "tenant".to_string(),
            Value::String(format!("tenant-{}", index % SERIAL_GROUP_COUNT)),
        ),
        ("score".to_string(), Value::Int64(1)),
    ])
}

fn json_len<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .expect("serialize test value")
        .len()
}

#[test]
fn should_account_serial_aggregate_groups_without_rescanning_buffered_rows() {
    // Arrange
    let parsed =
        crate::sql::parse_statement("SELECT tenant, COUNT(*) AS total FROM events GROUP BY tenant")
            .expect("parse grouped aggregate");
    let plan = build_logical_plan(&crate::catalog::Catalog::new(), &parsed)
        .expect("plan grouped aggregate");
    let specs = aggregate_specs(&plan);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let user_functions = HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        search_context: None,
        user_functions: &user_functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = (0..SERIAL_ROW_COUNT).map(tenant_row).collect::<Vec<_>>();
    let expected_peak = (0..SERIAL_GROUP_COUNT)
        .map(|group| {
            let values = vec![(
                "tenant".to_string(),
                Value::String(format!("tenant-{group}")),
            )];
            let group_rows = rows
                .iter()
                .skip(group)
                .step_by(SERIAL_GROUP_COUNT)
                .map(|row| json_len(row.entries()))
                .sum::<usize>();
            aggregate_group_signature(&values)
                .estimated_bytes()
                .saturating_add(json_len(&values))
                .saturating_add(group_rows)
        })
        .sum::<usize>();

    // Act
    let batches = aggregate_query_batches_serial(rows, &specs, &context)
        .expect("serial grouped aggregate should finish before the query deadline");

    // Assert
    let output = batches
        .iter()
        .flatten()
        .map(|row| row.entries().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(output.len(), SERIAL_GROUP_COUNT);
    let per_group = i64::try_from(SERIAL_ROW_COUNT / SERIAL_GROUP_COUNT)
        .expect("per-group row count should fit i64");
    assert!(output.iter().all(|entries| entries
        .iter()
        .any(|(_, value)| value == &Value::Int64(per_group))));
    assert_eq!(controls.peak_query_memory_bytes(), expected_peak);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn controls_with_budget(budget: usize) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn aggregate_plan(sql: &str) -> LogicalPlan {
    let parsed = crate::sql::parse_statement(sql).expect("parse aggregate");
    build_logical_plan(&crate::catalog::Catalog::new(), &parsed).expect("plan aggregate")
}

fn aggregate_context<'a>(
    plan: &'a LogicalPlan,
    user_functions: &'a HashMap<String, FunctionMeta>,
    controls: &'a QueryExecutionControls,
) -> AggregateExecutionContext<'a> {
    AggregateExecutionContext {
        plan,
        params: &[],
        search_context: None,
        user_functions,
        session: None,
        controls,
        after_partition_row: None,
    }
}

fn aggregate_call(plan: &LogicalPlan) -> &FunctionCall {
    let SelectItem::Function { function, .. } = &plan.projection[0] else {
        panic!("expected aggregate function projection");
    };
    function
}

#[test]
fn should_admit_statistical_numeric_state_growth_against_query_memory_budget() {
    // Arrange
    let plan = aggregate_plan("SELECT var_pop(value) FROM events");
    let functions = HashMap::new();
    let controls = controls_with_budget(0);
    let context = aggregate_context(&plan, &functions, &controls);
    let value = crate::types::extended::numeric::Numeric::from_i128(
        12_345_678_901_234_567_890_123_456_789_012_345_678_i128,
    );
    let rows = [BatchRow::new(vec![(
        "value".to_owned(),
        Value::Typed(crate::types::typed::TypedValue::Numeric(value)),
    )])];

    // Act
    let error = AggregateAccumulator::evaluate(aggregate_call(&plan), &rows, &context)
        .expect_err("growing numeric moments must respect the query budget");

    // Assert
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_retained_numeric_memory_when_later_growth_exceeds_budget() {
    // Arrange
    let plan = aggregate_plan("SELECT var_pop(value) FROM events");
    let function = aggregate_call(&plan);
    let first = Value::Typed(crate::types::typed::TypedValue::Numeric(
        crate::types::extended::numeric::parse_text("12345678901234567890")
            .expect("small NUMERIC input"),
    ));
    let large_input = format!("{}.5", "9".repeat(250));
    let second = Value::Typed(crate::types::typed::TypedValue::Numeric(
        crate::types::extended::numeric::parse_text(&large_input).expect("large NUMERIC input"),
    ));
    let mut measured = AggregateAccumulator::new(function);
    measured
        .update_values(function, first.clone(), None, None)
        .expect("first statistical transition");
    let first_retained_bytes = measured
        .retained_heap_bytes()
        .expect("first retained moment size");
    measured
        .update_values(function, second.clone(), None, None)
        .expect("second statistical transition");
    let second_retained_bytes = measured
        .retained_heap_bytes()
        .expect("second retained moment size");
    assert!(first_retained_bytes > 0);
    assert!(second_retained_bytes > first_retained_bytes);

    let controls = controls_with_budget(first_retained_bytes);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [first, second].map(|value| BatchRow::new(vec![("value".to_owned(), value)]));

    // Act
    let error = AggregateAccumulator::evaluate(function, &rows, &context)
        .expect_err("later numeric state growth must exceed the retained budget");

    // Assert
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(controls.peak_query_memory_bytes(), first_retained_bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_merge_exact_numeric_statistics_across_parallel_partitions() {
    // Arrange
    let plan = aggregate_plan("SELECT var_pop(value) FROM events");
    let specs = aggregate_specs(&plan);
    let functions = HashMap::new();
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let context = aggregate_context(&plan, &functions, &controls);
    let rows =
        [1, 3, 5, 7].map(|value| BatchRow::new(vec![("value".to_owned(), Value::Int64(value))]));
    let eligibility = parallel_aggregation_eligibility(&plan, &specs, &functions);

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 2)
        .expect("numeric statistical worker partitions");
    let mut merged = PartialAggregateGroup::new(Vec::new(), &specs);
    for partial in &partials {
        for group in partial.groups.values() {
            merged.merge(group).expect("merge exact Numeric moments");
        }
    }
    let values = merged
        .accumulators
        .into_iter()
        .map(AggregateAccumulator::finish)
        .collect::<Result<Vec<_>, _>>()
        .expect("finish exact parallel statistical state");
    drop(partials);

    // Assert
    assert_eq!(eligibility, Ok(()));
    let [Value::Typed(crate::types::typed::TypedValue::Numeric(variance))] = values.as_slice()
    else {
        panic!("integer variance should return NUMERIC, got {values:?}");
    };
    assert_eq!(
        variance.compare(&crate::types::extended::numeric::Numeric::from_i128(5)),
        std::cmp::Ordering::Equal
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_fall_back_to_row_order_for_parallel_float_statistics() {
    // Arrange
    let config = CassieRuntimeConfig {
        limits: CassieRuntimeLimits {
            parallel_aggregation_workers: 2,
            ..CassieRuntimeLimits::default()
        },
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    let plan = aggregate_plan("SELECT var_pop(value) FROM events");
    let functions = HashMap::new();
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = (0..2 * batch::DEFAULT_BATCH_SIZE)
        .map(|index| {
            let value = match index % 4 {
                0 => 1.0e16,
                1 => 1.0,
                2 => -1.0e16,
                _ => 3.0,
            };
            BatchRow::new(vec![("value".to_owned(), Value::Float64(value))])
        })
        .collect::<Vec<_>>();
    let serial = AggregateAccumulator::evaluate(aggregate_call(&plan), &rows, &context)
        .expect("serial float statistical aggregate");
    let before = cassie.runtime.snapshot();

    // Act
    let output = aggregate_query_batches_inner(&cassie, vec![rows], &context)
        .expect("parallel attempt should fall back to serial");
    let output_rows = batch::flatten_batches(output);
    let result = output_rows[0]
        .get("var_pop")
        .expect("statistical output column");
    let after = cassie.runtime.snapshot();

    // Assert
    assert_eq!(result, &serial);
    assert_eq!(
        after.parallel_aggregation.fallback_aggregations,
        before.parallel_aggregation.fallback_aggregations + 1
    );
    assert_eq!(
        after.parallel_aggregation.last_fallback_reason,
        "aggregate-state-requires-row-order"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_statistical_aggregate_before_publishing_a_partial_result() {
    // Arrange
    let plan = aggregate_plan("SELECT var_pop(value) FROM events");
    let functions = HashMap::new();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [BatchRow::new(vec![("value".to_owned(), Value::Int64(1))])];

    // Act
    let error = AggregateAccumulator::evaluate(aggregate_call(&plan), &rows, &context)
        .expect_err("cancelled statistical work cannot publish a result");

    // Assert
    assert!(error.to_string().contains("query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn payload_row(tenant: &str, payload: String) -> BatchRow {
    BatchRow::new(vec![
        ("tenant".to_string(), Value::String(tenant.to_string())),
        ("payload".to_string(), Value::String(payload)),
    ])
}

#[test]
fn should_keep_existing_group_reservation_when_growth_exceeds_budget() {
    // Arrange
    let controls = controls_with_budget(100);
    let mut memory = GroupMemory::new(&controls).expect("empty reservation");
    memory.add(60).expect("first growth fits the budget");

    // Act
    let result = memory.add(60);

    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 60);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_hold_partition_reservations_while_partial_groups_are_alive() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) AS total FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let user_functions = HashMap::new();
    let context = aggregate_context(&plan, &user_functions, &controls);
    let rows = (0..64).map(tenant_row).collect::<Vec<_>>();

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 2).expect("partial aggregate");

    // Assert
    assert_eq!(partials.len(), 2);
    assert!(controls.current_query_memory_bytes() > 0);
    drop(partials);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_charge_selected_min_max_text_to_partial_group_memory() {
    // Arrange
    const PAYLOAD_BYTES: usize = 64 * 1024;
    let plan = aggregate_plan("SELECT tenant, MAX(payload) AS widest FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let user_functions = HashMap::new();
    let context = aggregate_context(&plan, &user_functions, &controls);
    let rows = vec![
        payload_row("tenant-a", "a".to_string()),
        payload_row("tenant-a", "z".repeat(PAYLOAD_BYTES)),
    ];

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 1).expect("partial aggregate");

    // Assert
    assert!(
        controls.current_query_memory_bytes() >= PAYLOAD_BYTES,
        "charged {} bytes",
        controls.current_query_memory_bytes()
    );
    drop(partials);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_return_memory_limit_error_when_serial_groups_exceed_budget() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, COUNT(*) AS total FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(256);
    let user_functions = HashMap::new();
    let context = aggregate_context(&plan, &user_functions, &controls);
    let rows = (0..64).map(tenant_row).collect::<Vec<_>>();

    // Act
    let Err(error) = aggregate_query_batches_serial(rows, &specs, &context) else {
        panic!("serial aggregate should exceed the memory budget");
    };

    // Assert
    assert!(
        error.to_string().contains("query memory budget exceeded"),
        "{error}"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_return_memory_limit_error_when_parallel_partials_exceed_budget() {
    // Arrange
    let plan = aggregate_plan("SELECT tenant, MAX(payload) AS widest FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(4 * 1024);
    let user_functions = HashMap::new();
    let context = aggregate_context(&plan, &user_functions, &controls);
    let rows = (0..8)
        .map(|index| payload_row(&format!("tenant-{index}"), "x".repeat(1024)))
        .collect::<Vec<_>>();

    // Act
    let Err(error) = aggregate_partitions(&rows, &specs, &context, 2) else {
        panic!("parallel aggregate should exceed the memory budget");
    };

    // Assert
    assert!(
        error.to_string().contains("query memory budget exceeded"),
        "{error}"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_serial_array_extrema_comparison_keys() {
    // Arrange
    let plan = aggregate_plan("SELECT MIN(arr) FROM array_values");
    let controls = controls_with_budget(2_000);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let mut row = BatchRow::new(vec![(
        "arr".into(),
        Value::Json(serde_json::json!((0..100).collect::<Vec<_>>())),
    )]);
    row.set_data_types(
        vec![crate::types::DataType::Array(Box::new(
            crate::types::DataType::BigInt,
        ))]
        .into(),
    );
    let function = &aggregate_specs(&plan)[0].function;

    // Act
    let result = AggregateAccumulator::evaluate(function, &[row], &context);

    // Assert
    let error = result.expect_err("ARRAY comparison keys must respect the query budget");
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_array_window_order_keys() {
    // Arrange
    let plan = aggregate_plan("SELECT ROW_NUMBER() OVER (ORDER BY arr) FROM array_values");
    let controls = controls_with_budget(2_000);
    let functions = HashMap::new();
    let mut row = BatchRow::new(vec![(
        "arr".into(),
        Value::Json(serde_json::json!((0..100).collect::<Vec<_>>())),
    )]);
    row.set_data_types(
        vec![crate::types::DataType::Array(Box::new(
            crate::types::DataType::BigInt,
        ))]
        .into(),
    );

    // Act
    let result = super::super::window_exec::apply_window_functions(
        vec![vec![row]],
        &plan.projection,
        &[],
        None,
        &functions,
        None,
        &controls,
    );

    // Assert
    let error = result.expect_err("ARRAY window keys must respect the query budget");
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_evaluate_later_boolean_inputs_given_a_decisive_value_when_a_later_row_has_invalid_type() {
    // Arrange
    let plan = aggregate_plan("SELECT BOOL_AND(flag) FROM records");
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);

    // Act
    for (name, first) in [("bool_and", false), ("bool_or", true), ("every", false)] {
        let function = crate::sql::ast::FunctionCall {
            name: name.to_owned(),
            args: vec![Expr::Column("flag".to_owned())],
            ..Default::default()
        };
        let rows = [
            BatchRow::new(vec![("flag".to_owned(), Value::Bool(first))]),
            BatchRow::new(vec![("flag".to_owned(), Value::Int64(1))]),
        ];
        let result = AggregateAccumulator::evaluate(&function, &rows, &context);

        // Assert
        assert!(
            result.is_err(),
            "{name} must not skip the later invalid input"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_merge_boolean_row_groups_given_null_only_partitions_when_workers_finish() {
    // Arrange
    let plan = aggregate_plan("SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) FROM records");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [
        Value::Null,
        Value::Bool(true),
        Value::Null,
        Value::Bool(false),
    ]
    .into_iter()
    .map(|value| BatchRow::new(vec![("flag".to_owned(), value)]))
    .collect::<Vec<_>>();

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 4).expect("worker partitions");
    let mut merged = PartialAggregateGroup::new(Vec::new(), &specs);
    for partial in &partials {
        for group in partial.groups.values() {
            merged.merge(group).expect("merge real row-worker states");
        }
    }
    let values = merged
        .accumulators
        .into_iter()
        .map(AggregateAccumulator::finish)
        .collect::<Result<Vec<_>, _>>()
        .expect("finish merged Boolean states");
    drop(partials);

    // Assert
    assert_eq!(
        values,
        [Value::Bool(false), Value::Bool(true), Value::Bool(false)]
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_merge_any_value_partitions_while_accounting_retained_non_null_values() {
    // Arrange
    let plan = aggregate_plan("SELECT ANY_VALUE(item) AS selected FROM records");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [
        Value::Null,
        Value::String("first".to_owned()),
        Value::Null,
        Value::String("second".to_owned()),
    ]
    .into_iter()
    .map(|value| BatchRow::new(vec![("item".to_owned(), value)]))
    .collect::<Vec<_>>();

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 4).expect("worker partitions");
    let mut merged = PartialAggregateGroup::new(Vec::new(), &specs);
    for partial in &partials {
        for group in partial.groups.values() {
            merged.merge(group).expect("merge ANY_VALUE partial state");
        }
    }
    let retained = merged.accumulators[0]
        .retained_heap_bytes()
        .expect("retained ANY_VALUE heap bytes");
    let retained_accumulator = merged.accumulators[0].retained_bytes();
    let partial_memory_is_reserved = controls.current_query_memory_bytes() > 0;
    let selected = merged
        .accumulators
        .into_iter()
        .next()
        .expect("ANY_VALUE accumulator")
        .finish()
        .expect("finish merged ANY_VALUE state");
    drop(partials);

    // Assert
    assert!(matches!(
        selected,
        Value::String(ref value) if value == "first" || value == "second"
    ));
    assert!(retained > 0);
    assert!(retained_accumulator > std::mem::size_of::<AggregateAccumulator>());
    assert!(partial_memory_is_reserved);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_merge_integer_bitwise_aggregates_across_parallel_partitions() {
    // Arrange
    let plan = aggregate_plan("SELECT bit_and(value), bit_or(value), bit_xor(value) FROM records");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = [
        Value::Int64(7),
        Value::Int64(3),
        Value::Int64(5),
        Value::Null,
    ]
    .into_iter()
    .map(|value| BatchRow::new(vec![("value".to_owned(), value)]))
    .collect::<Vec<_>>();

    // Act
    let partials = aggregate_partitions(&rows, &specs, &context, 4).expect("worker partitions");
    let mut merged = PartialAggregateGroup::new(Vec::new(), &specs);
    for partial in &partials {
        for group in partial.groups.values() {
            merged.merge(group).expect("merge bitwise aggregate states");
        }
    }
    let values = merged
        .accumulators
        .into_iter()
        .map(AggregateAccumulator::finish)
        .collect::<Result<Vec<_>, _>>()
        .expect("finish merged bitwise states");
    drop(partials);

    // Assert
    assert_eq!(values, [Value::Int64(1), Value::Int64(7), Value::Int64(1)]);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
