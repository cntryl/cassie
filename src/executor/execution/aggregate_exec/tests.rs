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
