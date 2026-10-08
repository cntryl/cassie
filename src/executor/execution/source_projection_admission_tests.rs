//! Actual rich direct-copy and outer-scope admission controls.
use super::*;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

fn fixture(run: impl FnOnce(&Cassie)) {
    let path =
        std::env::temp_dir().join(format!("cassie-projection-copy-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    run(&cassie);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
}
fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn nested_type(depth: usize) -> DataType {
    (0..depth).fold(DataType::Text, |kind, _| DataType::Array(Box::new(kind)))
}
fn input(
    controls: &QueryExecutionControls,
    name: &str,
    value: Value,
    kind: DataType,
) -> (BatchRow, Arc<crate::runtime::QueryMemoryReservation>) {
    let row = BatchRow::new(vec![(name.into(), value)])
        .with_optional_data_types(Some(Arc::new(vec![kind])));
    let origin = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("source backing"))
            .expect("source admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&origin)))
        .retain_operator_memory(controls, Arc::clone(&origin))
        .expect("source operator");
    (row, origin)
}
fn plan(cassie: &Cassie, width: usize, source: &str) -> LogicalPlan {
    let statement =
        crate::sql::parse_statement("WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT n FROM c")
            .expect("SQL");
    let mut plan =
        super::super::build_logical_plan_in_session(cassie, None, &statement).expect("bound plan");
    plan.projection = (0..width)
        .map(|index| crate::sql::SelectItem::Column {
            name: source.into(),
            alias: Some(format!("x{index}")),
        })
        .collect();
    plan
}
fn actual_body(row: BatchRow) -> (BatchRow, usize) {
    let query = row.query_memory();
    let operator = row.operator_memory();
    let row = row.with_query_memory(None).with_operator_memory(None);
    let bytes = row
        .unleased_body_bytes()
        .expect("actual capacity/lookup/type backing");
    (
        row.with_query_memory(query).with_operator_memory(operator),
        bytes,
    )
}

#[test]
fn should_admit_repeated_nested_array_projection_capacity() {
    // Arrange
    fixture(|cassie| {
        for width in [3, 5, 9, 17] {
            let value = Value::Json(serde_json::json!([["s".repeat(768), "t".repeat(768)]]));
            let kind = nested_type(2);
            let plan = plan(cassie, width, "n");
            let controls = controls(4 * 1024 * 1024);
            let (row, parent) = input(&controls, "n", value.clone(), kind.clone());
            let before = controls.current_query_memory_bytes();
            // Act
            let mut output = apply_projection_phase(
                vec![vec![row]],
                &plan,
                &[],
                None,
                &HashMap::new(),
                None,
                &controls,
            )
            .expect("repeated rich projection");
            // Assert
            assert_eq!(output[0][0].entries().len(), width);
            assert!(output[0][0].entries().iter().all(|(_, v)| v == &value));
            assert_eq!(output[0][0].data_types(), vec![kind.clone(); width]);
            assert_eq!(output[0][0].get("x0"), Some(&value));
            let row = output[0].remove(0);
            let (row, actual) = actual_body(row);
            output[0].push(row);
            let delta = controls.current_query_memory_bytes() - before;
            println!(
                "ARRAY width={width} new_charge={delta} actual_body={actual} peak={}",
                controls.peak_query_memory_bytes()
            );
            assert!(
                delta >= actual,
                "whole reservation must cover actual retained rich backing"
            );
            let peak = controls.peak_query_memory_bytes();
            drop(parent);
            assert!(controls.current_query_memory_bytes() >= actual);
            drop(output);
            assert_eq!(controls.current_query_memory_bytes(), 0);
            let limited = super::projection_admission_tests::controls(peak - 1);
            let (row, parent) = input(&limited, "n", value, kind);
            let denied = apply_projection_phase(
                vec![vec![row]],
                &plan,
                &[],
                None,
                &HashMap::new(),
                None,
                &limited,
            );
            assert!(matches!(
                denied.map_err(crate::app::CassieError::from),
                Err(crate::app::CassieError::ResourceLimit(_))
            ));
            drop(parent);
            assert_eq!(limited.current_query_memory_bytes(), 0);
        }
    });
}

#[test]
fn should_admit_direct_outer_projection_copies() {
    // Arrange
    fixture(|cassie| {
        let cases = [
            (Value::String("s".repeat(65536)), DataType::Text),
            (
                Value::Vector(crate::types::Vector::new(vec![1.0; 16384])),
                DataType::Vector(16384),
            ),
            (Value::Null, nested_type(128)),
        ];
        let mut gaps = Vec::new();
        for (index, (value, kind)) in cases.into_iter().enumerate() {
            let plan = plan(cassie, 1, "o.payload");
            let controls = controls(4 * 1024 * 1024);
            let (outer, parent) = input(&controls, "o.payload", value.clone(), kind.clone());
            let outer = Arc::new(outer);
            let (inner, inner_parent) = input(&controls, "n", Value::Int64(1), DataType::BigInt);
            let inner = inner.with_outer_scope(Arc::clone(&outer));
            let before = controls.current_query_memory_bytes();
            // Act
            let mut output = apply_projection_phase(
                vec![vec![inner]],
                &plan,
                &[],
                None,
                &HashMap::new(),
                None,
                &controls,
            )
            .expect("outer copy");
            // Assert
            assert_eq!(output[0][0].get("x0"), Some(&value));
            if matches!(kind, DataType::Array(_)) {
                assert_eq!(output[0][0].data_types(), [kind.clone()]);
            }
            let row = output[0].remove(0);
            let (row, actual) = actual_body(row);
            output[0].push(row);
            let delta = controls.current_query_memory_bytes() - before;
            println!("outer case={index} delta={delta} actual={actual}");
            if delta < actual {
                gaps.push(index);
            }
            drop(outer);
            drop(parent);
            drop(inner_parent);
            assert!(controls.current_query_memory_bytes() > 0);
            drop(output);
            assert_eq!(controls.current_query_memory_bytes(), 0);
            let limited = super::projection_admission_tests::controls(before + actual - 1);
            let (outer, parent) = input(&limited, "o.payload", value, kind);
            let (inner, inner_parent) = input(&limited, "n", Value::Int64(1), DataType::BigInt);
            let batches = vec![vec![inner.with_outer_scope(Arc::new(outer))]];
            let denied =
                reserve_projection_output_before_building(&limited, &batches, &plan.projection);
            if !matches!(
                denied.map_err(crate::app::CassieError::from),
                Err(crate::app::CassieError::ResourceLimit(_))
            ) {
                gaps.push(index + 10);
            }
            drop(batches);
            drop(parent);
            drop(inner_parent);
            assert_eq!(limited.current_query_memory_bytes(), 0);
        }
        assert!(
            gaps.is_empty(),
            "outer copy charge/admission gaps: {gaps:?}"
        );
    });
}

#[test]
fn should_preserve_public_lateral_outer_direct_values_and_types() {
    // Arrange
    fixture(|cassie| {
        let session = cassie.create_session("tester", None);
        let payload = "s".repeat(65536);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE outer_copy (payload TEXT, a TEXT[])",
                vec![],
            )
            .expect("table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO outer_copy (payload, a) VALUES ($1, NULL)",
                vec![Value::String(payload.clone())],
            )
            .expect("row");
        cassie
            .execute_sql(&session, "CREATE TABLE inner_copy (n BIGINT)", vec![])
            .expect("inner table");
        cassie
            .execute_sql(&session, "INSERT INTO inner_copy (n) VALUES (1)", vec![])
            .expect("inner row");
        for (source, prefix) in [
            ("outer_copy", "outer_copy"),
            ("outer_copy AS \"O\"", "\"O\""),
        ] {
            let sql = format!("SELECT q.x, q.a FROM {source} JOIN LATERAL (SELECT {prefix}.payload AS x, {prefix}.a AS a FROM (SELECT DISTINCT n FROM inner_copy) AS owned) AS q ON true");
            // Act
            let result = cassie
                .execute_sql(&session, &sql, vec![])
                .expect("supported lateral projection");
            // Assert
            assert_eq!(
                result.rows,
                vec![vec![Value::String(payload.clone()), Value::Null]]
            );
            assert_eq!(result.columns[0].type_oid, 25);
            assert_eq!(result.columns[1].type_oid, 34025);
        }
    });
}
