//! Literal complete-input join semantics across independently owned source partitions.
use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Instant;

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("join qualification directory cleanup failed: {error}");
            } else {
                panic!("join qualification directory cleanup failed: {error}");
            }
        }
    }
}
fn fixture(run: impl FnOnce(&SourceExecutionEnv<'_>)) {
    let directory =
        Directory(std::env::temp_dir().join(format!("cassie-761-join-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&directory.0).expect("private fixture directory");
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024 * 1024;
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let cassie =
        Cassie::new_with_data_dir_and_config(&directory.0, config).expect("controlled Cassie");
    let functions = HashMap::new();
    run(&SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    });
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let path = directory.0.clone();
    drop(directory);
    assert!(!path.exists());
}
fn row(side: &str, id: Option<i64>, key: Option<i64>) -> BatchRow {
    BatchRow::new(vec![
        (format!("{side}.id"), id.map_or(Value::Null, Value::Int64)),
        (format!("{side}.key"), key.map_or(Value::Null, Value::Int64)),
    ])
    .with_optional_data_types(Some(Arc::new(vec![DataType::BigInt, DataType::BigInt])))
}
fn partitioned(
    side: &str,
    values: &[(i64, Option<i64>)],
    split: usize,
    controls: &QueryExecutionControls,
) -> (Vec<BatchRow>, Vec<Weak<QueryMemoryReservation>>) {
    let mut owners = Vec::new();
    let partitions = [&values[..split], &values[split..]]
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
                .map(|(id, key)| {
                    row(side, Some(*id), *key)
                        .with_query_memory(Some(Arc::clone(&marker)))
                        .retain_operator_memory(controls, Arc::clone(&marker))
                        .expect("independent prior operator owner")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // The selected join adapter consumes complete rows, not independently streamed fragments.
    (partitions.into_iter().flatten().collect(), owners)
}
fn integer(row: &BatchRow, name: &str) -> Option<i64> {
    match row.get(name).expect("declared field") {
        Value::Null => None,
        Value::Int64(value) => Some(*value),
        _ => panic!("declared BIGINT carrier"),
    }
}
fn expected_pairs(kind: JoinKind) -> Vec<(Option<i64>, Option<i64>)> {
    if matches!(kind, JoinKind::Left) {
        vec![
            (Some(1), Some(10)),
            (Some(1), Some(11)),
            (Some(2), Some(10)),
            (Some(2), Some(11)),
            (Some(3), None),
            (Some(4), Some(13)),
            (Some(5), None),
        ]
    } else {
        vec![
            (Some(1), Some(10)),
            (Some(1), Some(11)),
            (Some(2), Some(10)),
            (Some(2), Some(11)),
            (Some(4), Some(13)),
        ]
    }
}

#[test]
fn should_preserve_typed_join_pairs_across_source_partition_grid() {
    // Arrange
    fixture(|env| {
        let left_values = [
            (1, Some(1)),
            (2, Some(1)),
            (3, None),
            (4, Some(0)),
            (5, Some(3)),
        ];
        let right_values = [
            (10, Some(1)),
            (11, Some(1)),
            (12, None),
            (13, Some(0)),
            (14, Some(2)),
        ];
        let on = Expr::Binary {
            left: Box::new(Expr::Column("left.key".into())),
            op: BinaryOp::Eq,
            right: Box::new(Expr::Column("right.key".into())),
        };
        for kind in [JoinKind::Left, JoinKind::Inner] {
            let expected = expected_pairs(kind);
            for left_split in 0..=5 {
                for right_split in 0..=5 {
                    let (left, left_owners) =
                        partitioned("left", &left_values, left_split, env.controls);
                    let (right, right_owners) =
                        partitioned("right", &right_values, right_split, env.controls);
                    let templates = (row("left", None, None), row("right", None, None));
                    let input =
                        reserve_join_rows(env, &left, &right).expect("complete input admission");
                    let before = env.cassie.runtime.snapshot().joins;
                    // Act
                    let joined = execute_loaded_join(
                        env,
                        JoinRowsSpec {
                            kind,
                            sources: None,
                            on: &on,
                            left_rows: &left,
                            right_rows: &right,
                            left_template: &templates.0,
                            right_template: &templates.1,
                            row_budget: None,
                        },
                        &["left.key".into()],
                        &["right.key".into()],
                    )
                    .expect("selected typed join");
                    let (output, _) =
                        finish_retained_join(env, joined).expect("completed owner transfer");
                    drop(left);
                    drop(right);
                    drop(input);
                    // Assert
                    let mut pairs = output
                        .iter()
                        .flatten()
                        .map(|row| (integer(row, "left.id"), integer(row, "right.id")))
                        .collect::<Vec<_>>();
                    pairs.sort();
                    assert_eq!(pairs, expected);
                    let after = env.cassie.runtime.snapshot().joins;
                    assert_eq!(after.last_strategy, "typed_hash");
                    assert_eq!(
                        after.vectorized_build_rows_total - before.vectorized_build_rows_total,
                        4
                    );
                    assert_eq!(
                        after.vectorized_probe_rows_total - before.vectorized_probe_rows_total,
                        5
                    );
                    assert_eq!(after.matched_rows_total - before.matched_rows_total, 5);
                    assert_eq!(
                        after.output_rows_total - before.output_rows_total,
                        u64::try_from(expected.len()).expect("finite rows")
                    );
                    assert!(output
                        .iter()
                        .flatten()
                        .all(|row| row.query_memory().is_some()
                            && row.operator_memory().is_some()
                            && row.data_types()
                                == [
                                    DataType::BigInt,
                                    DataType::BigInt,
                                    DataType::BigInt,
                                    DataType::BigInt
                                ]));
                    assert!(env.controls.current_query_memory_bytes() > 0);
                    drop(output);
                    assert!(left_owners
                        .iter()
                        .chain(&right_owners)
                        .all(|owner| owner.upgrade().is_none()));
                    assert_eq!(env.controls.current_query_memory_bytes(), 0);
                }
            }
        }
    });
}
