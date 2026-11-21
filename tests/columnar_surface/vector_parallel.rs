//! Issue #1358, slice S5: parallel waves over the vector path return exactly
//! what the serial read returns, report why they stay serial, and stay off by
//! default.

use super::pair::{with_pair_config, Pair};
use cassie::midge::adapter::ColumnVectorReadSnapshot;
use cassie::types::Value;

const ROWS: usize = 1_300;
const CHUNK: usize = 260;
const COLUMNS: &str = "g TEXT, k INT, v BIGINT, f FLOAT, a BIGINT";

/// Row `index`: NULL keys and inputs, and floats whose sum depends on order.
fn row(index: usize) -> String {
    let g = if index % 17 == 3 {
        "NULL".to_owned()
    } else {
        format!("'g{}'", index % 6)
    };
    let k = if index % 11 == 5 {
        "NULL".to_owned()
    } else {
        (index % 4).to_string()
    };
    let v = if index % 13 == 2 {
        "NULL".to_owned()
    } else {
        (index % 97).to_string()
    };
    let f = match index % 5 {
        0 => "1.0e16".to_owned(),
        1 => "1.1".to_owned(),
        2 => "-1.0e16".to_owned(),
        3 => "NULL".to_owned(),
        _ => "0.3".to_owned(),
    };
    format!("({g}, {k}, {v}, {f}, {})", index * 10)
}

fn load(pair: &Pair, rows: usize) {
    pair.run(&format!(
        "CREATE TABLE t ({COLUMNS}) WITH (storage = column_store)"
    ));
    for start in (0..rows).step_by(CHUNK) {
        let values = (start..(start + CHUNK).min(rows))
            .map(row)
            .collect::<Vec<_>>()
            .join(",");
        pair.run(&format!("INSERT INTO t VALUES {values}"));
    }
    pair.cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics("t")
        .expect("summaries");
}

fn vector(pair: &Pair) -> ColumnVectorReadSnapshot {
    pair.cassie.midge.column_vector_reads_for_diagnostics()
}

fn at_workers(workers: usize, test: impl FnOnce(&Pair)) {
    with_pair_config(
        |config| {
            config.limits.parallel_aggregation_workers = workers;
            config.limits.parallel_scan_workers = workers;
        },
        test,
    );
}

/// The rows each statement returns at `workers`, in the order returned.
fn results_at(workers: usize, statements: &[&str]) -> Vec<Vec<Vec<Value>>> {
    let mut results = Vec::new();
    at_workers(workers, |pair| {
        load(pair, ROWS);
        results = statements.iter().map(|sql| pair.run(sql)).collect();
    });
    results
}

/// Floats compare by bit pattern, so a reordered sum cannot hide.
fn bits(rows: &[Vec<Value>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    Value::Float64(float) => format!("f64:{:#x}", float.to_bits()),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn assert_equal_to_serial(
    statements: &[&str],
    serial: &[Vec<Vec<Value>>],
    parallel: &[Vec<Vec<Vec<Value>>>],
) {
    for (workers, actual_set) in [2, 4].into_iter().zip(parallel) {
        for ((sql, expected), actual) in statements.iter().zip(serial).zip(actual_set) {
            assert_eq!(bits(actual), bits(expected), "{sql} at {workers} workers");
        }
    }
}

#[test]
fn should_equal_the_serial_result_for_ungrouped_integer_aggregates() {
    // Arrange
    let statements = [
        "SELECT COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v) FROM t",
        "SELECT COUNT(*), SUM(v), MIN(k), MAX(k) FROM t WHERE v > 20 AND k IS NOT NULL",
    ];

    // Act
    let serial = results_at(1, &statements);
    let parallel = [2, 4].map(|workers| results_at(workers, &statements));

    // Assert
    assert_equal_to_serial(&statements, &serial, &parallel);
}

#[test]
fn should_equal_the_serial_result_for_ungrouped_float_aggregates_bit_for_bit() {
    // Arrange
    let statements = [
        "SELECT SUM(f), AVG(f), COUNT(f), MIN(f), MAX(f) FROM t",
        "SELECT SUM(f), AVG(f) FROM t WHERE v < 60",
    ];

    // Act
    let serial = results_at(1, &statements);
    let parallel = [2, 4].map(|workers| results_at(workers, &statements));

    // Assert
    assert_equal_to_serial(&statements, &serial, &parallel);
}

#[test]
fn should_equal_the_serial_result_with_group_order_for_grouped_integer_aggregates() {
    // Arrange
    let statements = [
        "SELECT g, COUNT(*), COUNT(v), SUM(v), MIN(v), MAX(v) FROM t GROUP BY g",
        "SELECT k, g, SUM(v), MAX(a) FROM t WHERE v IS NOT NULL GROUP BY k, g",
    ];

    // Act
    let serial = results_at(1, &statements);
    let parallel = [2, 4].map(|workers| results_at(workers, &statements));

    // Assert
    assert_equal_to_serial(&statements, &serial, &parallel);
}

#[test]
fn should_equal_the_serial_result_for_grouped_float_aggregates_bit_for_bit() {
    // Arrange
    let statements = [
        "SELECT g, SUM(f), AVG(f), COUNT(f) FROM t GROUP BY g",
        "SELECT k, SUM(f), SUM(v), MIN(f) FROM t WHERE a > 100 GROUP BY k",
    ];

    // Act
    let serial = results_at(1, &statements);
    let parallel = [2, 4].map(|workers| results_at(workers, &statements));

    // Assert
    assert_equal_to_serial(&statements, &serial, &parallel);
}

#[test]
fn should_return_projection_rows_in_the_serial_row_id_order() {
    // Arrange
    let statements = [
        "SELECT v, f, a FROM t WHERE v >= 10",
        "SELECT g, k FROM t WHERE f IS NOT NULL",
        "SELECT a FROM t",
    ];

    // Act
    let serial = results_at(1, &statements);
    let parallel = [2, 4].map(|workers| results_at(workers, &statements));

    // Assert
    assert_equal_to_serial(&statements, &serial, &parallel);
}

#[test]
fn should_split_a_segmented_table_into_one_partition_per_segment() {
    // Arrange
    at_workers(4, |pair| {
        load(pair, ROWS);
        let before = vector(pair);

        // Act
        pair.run("SELECT COUNT(*), SUM(v) FROM t");
        let delta = vector(pair).since(before);

        // Assert
        assert_eq!(delta.parallel_scans, 1);
        assert_eq!(delta.workers, 4);
        assert!(delta.partitions >= 4, "{} partitions", delta.partitions);
        assert_eq!(delta.row_documents_built, 0);
        assert_eq!(delta.last_serial_label, "");
    });
}

#[test]
fn should_read_fewer_partitions_when_pruning_excludes_segments() {
    // Arrange
    let sql = "SELECT COUNT(*), SUM(v) FROM t WHERE a >= 7000";
    let expected = at_serial(sql);
    at_workers(2, |pair| {
        load(pair, ROWS);
        let before = vector(pair);
        pair.run("SELECT COUNT(*), SUM(v) FROM t");
        let unpruned = vector(pair).since(before);
        let before = vector(pair);

        // Act
        let rows = pair.run(sql);
        let pruned = vector(pair).since(before);

        // Assert
        assert_eq!(rows, expected);
        assert!(pruned.segments_pruned > 0);
        assert_eq!(
            pruned.partitions + pruned.segments_pruned,
            unpruned.partitions
        );
    });
}

fn at_serial(sql: &str) -> Vec<Vec<Value>> {
    results_at(1, &[sql]).remove(0)
}

fn parallel_scans_of(sql: &str) -> u64 {
    let mut scans = 0;
    at_workers(4, |pair| {
        load(pair, ROWS);
        let before = vector(pair);
        pair.run(sql);
        scans = vector(pair).since(before).parallel_scans;
    });
    scans
}

#[test]
fn should_run_a_projection_in_waves() {
    // Arrange
    let sql = "SELECT v, f FROM t WHERE v >= 10";

    // Act
    let scans = parallel_scans_of(sql);

    // Assert
    assert_eq!(scans, 1);
}

#[test]
fn should_run_an_ungrouped_float_aggregate_in_waves() {
    // Arrange
    let sql = "SELECT SUM(f), AVG(f) FROM t";

    // Act
    let scans = parallel_scans_of(sql);

    // Assert
    assert_eq!(scans, 1);
}

#[test]
fn should_run_a_grouped_float_aggregate_in_waves() {
    // Arrange
    let sql = "SELECT g, SUM(f) FROM t GROUP BY g";

    // Act
    let scans = parallel_scans_of(sql);

    // Assert
    assert_eq!(scans, 1);
}

#[test]
fn should_report_the_worker_limit_one_label_by_default() {
    // Arrange
    at_workers(1, |pair| {
        load(pair, ROWS);
        let before = vector(pair);

        // Act
        pair.run("SELECT g, SUM(v) FROM t GROUP BY g");
        let delta = vector(pair).since(before);

        // Assert
        assert_eq!(delta.last_serial_label, "column-vector-worker-limit-one");
        assert_eq!(delta.partitions, 0);
        assert_eq!(delta.parallel_scans, 0);
    });
}

#[test]
fn should_report_the_single_partition_label_for_a_table_without_segments() {
    // Arrange
    at_workers(4, |pair| {
        load(pair, 3);
        let before = vector(pair);

        // Act
        let rows = pair.run("SELECT COUNT(*) FROM t");
        let delta = vector(pair).since(before);

        // Assert
        assert_eq!(rows, vec![vec![Value::Int64(3)]]);
        assert_eq!(delta.last_serial_label, "column-vector-single-partition");
        assert_eq!(delta.parallel_scans, 0);
    });
}

#[test]
fn should_read_serially_with_the_staged_label_in_a_session_with_staged_rows() {
    // Arrange
    at_workers(4, |pair| {
        load(pair, ROWS);
        pair.run("BEGIN");
        pair.run("INSERT INTO t VALUES ('staged', 1, 1000000, 1.5, 7)");
        let before = vector(pair);

        // Act
        let rows = pair.run("SELECT COUNT(*), MAX(v) FROM t");
        let delta = vector(pair).since(before);
        pair.run("ROLLBACK");

        // Assert
        let counted = i64::try_from(ROWS).expect("rows") + 1;
        assert_eq!(
            rows,
            vec![vec![Value::Int64(counted), Value::Int64(1_000_000)]]
        );
        assert_eq!(delta.last_serial_label, "column-vector-staged-serial");
        assert_eq!(delta.parallel_scans, 0);
        assert_eq!(delta.staged_rows_merged, 1);
    });
}

#[test]
fn should_report_partition_deltas_in_explain_analyze() {
    // Arrange
    at_workers(4, |pair| {
        load(pair, ROWS);

        // Act
        let plan = match &pair.run("EXPLAIN ANALYZE SELECT g, SUM(v) FROM t GROUP BY g")[0][0] {
            Value::String(plan) => plan.clone(),
            _ => panic!("EXPLAIN returned a non-text plan"),
        };

        // Assert
        let token = |name: &str| {
            plan.split_ascii_whitespace()
                .find_map(|part| part.strip_prefix(&format!("{name}=")))
                .unwrap_or_else(|| panic!("token {name} absent from {plan}"))
                .to_owned()
        };
        assert!(
            token("column_vector_partitions_delta")
                .parse::<u64>()
                .expect("number")
                >= 4
        );
        assert_eq!(token("column_vector_workers_delta"), "4");
        assert_eq!(token("column_vector_serial_label"), "none");
    });
}
