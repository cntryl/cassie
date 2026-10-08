//! Independent literals on real batch boundaries and source-owner partitions.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryMemoryReservation;
use std::sync::{Arc, Weak};
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn partitions(
    values: &[Option<i64>],
    split: usize,
    controls: &QueryExecutionControls,
) -> (Vec<Batch>, Vec<Weak<QueryMemoryReservation>>) {
    let types = Arc::new(vec![DataType::BigInt]);
    let mut owners = Vec::new();
    let batches = [&values[..split], &values[split..]]
        .into_iter()
        .map(|part| {
            if part.is_empty() {
                return Vec::new();
            }
            let marker = Arc::new(
                controls
                    .reserve_query_memory(256)
                    .expect("source lease marker"),
            );
            owners.push(Arc::downgrade(&marker));
            part.iter()
                .map(|value| {
                    BatchRow::new(vec![("n".into(), value.map_or(Value::Null, Value::Int64))])
                        .with_optional_data_types(Some(Arc::clone(&types)))
                        .with_query_memory(Some(Arc::clone(&marker)))
                })
                .collect()
        })
        .collect();
    (batches, owners)
}

fn cells(rows: &[BatchRow]) -> Vec<Option<i64>> {
    rows.iter()
        .map(|row| match &row.entries()[0].1 {
            Value::Null => None,
            Value::Int64(value) => Some(*value),
            _ => panic!("declared BIGINT carrier"),
        })
        .collect()
}

fn set(operator: &str) -> SelectSet {
    let statement = crate::sql::parse_statement(&format!("SELECT 1 {operator} SELECT 1"))
        .expect("selected set operator");
    *crate::executor::execution::build_logical_plan(&crate::catalog::Catalog::new(), &statement)
        .expect("set plan")
        .set
        .expect("set node")
}

#[test]
fn should_preserve_distinct_winners_across_default_batch_boundary() {
    // Arrange
    let values = (0..1025)
        .map(|index| {
            if index % 5 == 0 {
                None
            } else {
                Some(index % 4)
            }
        })
        .collect::<Vec<_>>();
    for split in [0, 1, 512, 1023, 1024, 1025] {
        let controls = controls();
        let (batches, owners) = partitions(&values, split, &controls);

        // Act
        let output = distinct_batches(batches, &controls).expect("actual DISTINCT batch boundary");
        let rows = output.into_iter().flatten().collect::<Vec<_>>();

        // Assert
        assert_eq!(cells(&rows), vec![None, Some(1), Some(2), Some(3), Some(0)]);
        assert!(rows.iter().all(|row| row.operator_memory().is_some()
            && row.query_memory().is_some()
            && row.data_types() == [DataType::BigInt]));
        assert_eq!(
            owners
                .iter()
                .filter(|owner| owner.upgrade().is_some())
                .count(),
            if split > 0 && split < 5 { 2 } else { 1 }
        );
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "native_primitive_keys"))
        );
        assert!(controls.current_query_memory_bytes() > 0);
        drop(rows);
        assert!(owners.iter().all(|owner| owner.upgrade().is_none()));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_three_branch_set_bag_across_owner_partition_grid() {
    // Arrange
    let union = set("UNION");
    let intersect = set("INTERSECT");
    for a in 0..=3 {
        for b in 0..=4 {
            for c in 0..=2 {
                let controls = controls();
                let (left, left_owners) = partitions(&[Some(1), Some(1), None], a, &controls);
                let (middle, middle_owners) =
                    partitions(&[Some(1), Some(2), None, None], b, &controls);
                let (right, right_owners) = partitions(&[Some(2), None], c, &controls);
                let names = ["n".to_owned()];

                // Act
                let inner = apply_set_operation(
                    middle.into_iter().flatten().collect(),
                    right.into_iter().flatten().collect(),
                    &names,
                    &intersect,
                    &controls,
                )
                .expect("right INTERSECT branch");
                let output = apply_set_operation(
                    left.into_iter().flatten().collect(),
                    inner,
                    &names,
                    &union,
                    &controls,
                )
                .expect("left UNION branch");
                let mut actual = cells(&output);
                actual.sort();

                // Assert
                assert_eq!(
                    actual,
                    vec![None, Some(1), Some(2)],
                    "partitions {a}/{b}/{c}"
                );
                assert!(output
                    .iter()
                    .all(|row| row.operator_memory().is_some() && row.query_memory().is_some()));
                assert!(controls.current_query_memory_bytes() > 0);
                assert_eq!(
                    crate::executor::typed_batch::relational_diagnostics::last_path(),
                    Some(("set", "native_primitive_keys"))
                );
                drop(output);
                assert!(left_owners
                    .iter()
                    .chain(&middle_owners)
                    .chain(&right_owners)
                    .all(|owner| owner.upgrade().is_none()));
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}

#[test]
fn should_preserve_two_branch_set_multiplicity_across_owner_partitions() {
    // Arrange
    for (operator, expected) in [
        (
            "UNION ALL",
            vec![None, None, None, Some(1), Some(1), Some(1), Some(2)],
        ),
        ("UNION", vec![None, Some(1), Some(2)]),
        ("INTERSECT", vec![None, Some(1)]),
        ("EXCEPT", vec![]),
    ] {
        let set = set(operator);
        for a in 0..=3 {
            for b in 0..=4 {
                let controls = controls();
                let (left, left_owners) = partitions(&[Some(1), Some(1), None], a, &controls);
                let (right, right_owners) =
                    partitions(&[Some(1), Some(2), None, None], b, &controls);

                // Act
                let output = apply_set_operation(
                    left.into_iter().flatten().collect(),
                    right.into_iter().flatten().collect(),
                    &["n".into()],
                    &set,
                    &controls,
                )
                .expect("actual set owner boundary");
                let mut actual = cells(&output);
                actual.sort();

                // Assert
                assert_eq!(actual, expected, "{operator} partitions {a}/{b}");
                drop(output);
                assert!(left_owners
                    .iter()
                    .chain(&right_owners)
                    .all(|owner| owner.upgrade().is_none()));
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}

#[test]
fn should_preserve_signed_zero_first_winner_with_exact_numeric_classes() {
    // Arrange
    let values = [
        Value::Float64(-0.0),
        Value::Int64(0),
        Value::Float64(0.0),
        Value::Int64(9_007_199_254_740_993),
        Value::Float64(9_007_199_254_740_992.0),
    ];
    for split in 0..=values.len() {
        let controls = controls();
        let marker = Arc::new(
            controls
                .reserve_query_memory(256)
                .expect("source owner marker"),
        );
        let weak = Arc::downgrade(&marker);
        let rows = values
            .iter()
            .map(|value| {
                BatchRow::new(vec![("n".into(), value.clone())])
                    .with_query_memory(Some(Arc::clone(&marker)))
            })
            .collect::<Vec<_>>();
        let mut left = rows;
        let right = left.split_off(split);
        drop(marker);
        // Act
        let output = distinct_batches(vec![left, right], &controls)
            .expect("exact mixed numeric semantic keys");
        // Assert
        let rows = output.iter().flatten().collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        match rows[0].get("n").expect("first winner") {
            Value::Float64(value) => assert_eq!(value.to_bits(), 0x8000_0000_0000_0000),
            _ => panic!("raw negative-zero winner carrier"),
        }
        assert_eq!(rows[1].get("n"), Some(&Value::Int64(9_007_199_254_740_993)));
        assert_eq!(
            rows[2].get("n"),
            Some(&Value::Float64(9_007_199_254_740_992.0))
        );
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "bounded_semantic_keys"))
        );
        assert!(weak.upgrade().is_some());
        drop(rows);
        drop(output);
        assert!(weak.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_declared_array_null_identity_across_batch_splits() {
    // Arrange
    let values = [
        Value::Null,
        Value::Json(serde_json::json!([])),
        Value::Json(serde_json::json!(["x", null])),
        Value::Json(serde_json::json!([])),
        Value::Json(serde_json::json!(["x", null])),
        Value::Null,
    ];
    for split in 0..=values.len() {
        let controls = controls();
        let types = Arc::new(vec![DataType::Array(Box::new(DataType::Text))]);
        let marker = Arc::new(
            controls
                .reserve_query_memory(256)
                .expect("rich source marker"),
        );
        let weak = Arc::downgrade(&marker);
        let mut left = values
            .iter()
            .map(|value| {
                BatchRow::new(vec![("a".into(), value.clone())])
                    .with_optional_data_types(Some(Arc::clone(&types)))
                    .with_query_memory(Some(Arc::clone(&marker)))
            })
            .collect::<Vec<_>>();
        let right = left.split_off(split);
        drop(marker);
        // Act
        let output =
            distinct_batches(vec![left, right], &controls).expect("selected ARRAY fallback");
        // Assert
        let rows = output.iter().flatten().collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get("a"), Some(&Value::Null));
        assert_eq!(rows[1].get("a"), Some(&Value::Json(serde_json::json!([]))));
        assert_eq!(
            rows[2].get("a"),
            Some(&Value::Json(serde_json::json!(["x", null])))
        );
        assert!(rows.iter().all(|row| row.data_types() == types.as_slice()
            && row.operator_memory().is_some()
            && row.query_memory().is_some()));
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "bounded_semantic_keys"))
        );
        assert!(weak.upgrade().is_some());
        drop(rows);
        drop(output);
        assert!(weak.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
