use std::collections::HashMap;
use std::time::Instant;

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::build_logical_plan;

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
    }
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
