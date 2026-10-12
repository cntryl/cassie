//! Mixed numeric joins preserve the independently observed PostgreSQL bags.
use super::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled, OperatorSwitchingEnabled};
use cassie::types::Value;
use serde_json::Value as Metrics;

const SETUP: &[&str] = &[
    "CREATE TABLE mix_int2_l (id INT, k SMALLINT)",
    "INSERT INTO mix_int2_l (id,k) VALUES (1,7), (2,7), (3,32767), (4,-32768), (5,NULL)",
    "CREATE TABLE mix_int2_r (id INT, k FLOAT)",
    "INSERT INTO mix_int2_r (id,k) VALUES (1,7.0), (2,7.0), (3,32767.0), (4,-32768.0), (5,NULL)",
    "CREATE TABLE mix_int4_l (id INT, k INT)",
    "INSERT INTO mix_int4_l (id,k) VALUES (1,16777217), (2,16777217), (3,2147483647), (4,-2147483648), (5,NULL)",
    "CREATE TABLE mix_int4_r (id INT, k FLOAT)",
    "INSERT INTO mix_int4_r (id,k) VALUES (1,16777217.0), (2,16777217.0), (3,2147483647.0), (4,-2147483648.0), (5,NULL)",
    "CREATE TABLE mix_int8_l (id INT, k BIGINT)",
    "INSERT INTO mix_int8_l (id,k) VALUES (1,9007199254740993), (2,9007199254740992), (3,9007199254740991), (4,NULL), (5,-9007199254740993), (6,9223372036854775807)",
    "CREATE TABLE mix_int8_r (id INT, k FLOAT)",
    "INSERT INTO mix_int8_r (id,k) VALUES (1,9007199254740992.0), (2,9007199254740992.0), (3,9007199254740991.0), (4,NULL), (5,-9007199254740992.0), (6,9223372036854775808.0)",
];

fn fixture(label: &str, vectorized: bool, switching: bool) -> SqlFixture {
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024 * 1024;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.vectorized_joins_enabled = vectorized;
    config.limits.vectorized_join_batch_size = 2;
    config.limits.adaptive_execution_enabled = false;
    config.limits.parallel_scan_workers = 1;
    config.limits.max_result_rows = 64;
    config.limits.operator_switching_enabled = if switching {
        OperatorSwitchingEnabled::enabled()
    } else {
        OperatorSwitchingEnabled::disabled()
    };
    config.limits.operator_switch_join_row_threshold = 1;
    sql_fixture_with_config(label, SETUP, config)
}

fn count(metrics: &Metrics, section: &str, name: &str) -> u64 {
    metrics[section][name]
        .as_u64()
        .expect("present numeric counter")
}

fn delta(before: &Metrics, after: &Metrics, section: &str, name: &str) -> u64 {
    count(after, section, name)
        .checked_sub(count(before, section, name))
        .expect("monotonic counter")
}

struct Observation {
    rows: Vec<Vec<Value>>,
    columns: Vec<String>,
    before: Metrics,
    after: Metrics,
}

fn execute(fixture: &SqlFixture, sql: &str) -> Observation {
    let before = fixture.cassie.metrics();
    let result = fixture.execute(sql).expect("selected join succeeds");
    let after = fixture.cassie.metrics();
    let rows = result.rows().to_vec();
    let columns = result
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect();
    drop(result);
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0,
        "managed result retires with result caching disabled"
    );
    Observation {
        rows,
        columns,
        before,
        after,
    }
}

fn query(family: &str, left: bool, reverse: bool, ordered: bool) -> String {
    let left_table = format!("mix_{family}_l");
    let right_table = format!("mix_{family}_r");
    let join = if left { "LEFT JOIN" } else { "INNER JOIN" };
    let equality = if reverse {
        format!("{right_table}.k = {left_table}.k")
    } else {
        format!("{left_table}.k = {right_table}.k")
    };
    let suffix = if ordered {
        format!(" ORDER BY {left_table}.id, {right_table}.id")
    } else {
        // No aliases or ORDER BY: eligible for the real bounded collection path.
        " LIMIT 64".into()
    };
    format!(
        "SELECT {left_table}.id AS left_id, {right_table}.id AS right_id, \
         {left_table}.k AS integer_value, {right_table}.k AS float_value \
         FROM {left_table} {join} {right_table} ON {equality}{suffix}"
    )
}

fn pg_rows(family: &str, left: bool) -> Vec<Vec<Value>> {
    // Literal retained PostgreSQL rows; this does not calculate equality/coercion.
    let tuples: Vec<(i64, i64, i64, f64)> = match family {
        "int2" => vec![
            (1, 1, 7, 7.0),
            (1, 2, 7, 7.0),
            (2, 1, 7, 7.0),
            (2, 2, 7, 7.0),
            (3, 3, 32767, 32767.0),
            (4, 4, -32768, -32768.0),
        ],
        "int4" => vec![
            (1, 1, 16777217, 16777217.0),
            (1, 2, 16777217, 16777217.0),
            (2, 1, 16777217, 16777217.0),
            (2, 2, 16777217, 16777217.0),
            (3, 3, 2147483647, 2147483647.0),
            (4, 4, -2147483648, -2147483648.0),
        ],
        "int8" => vec![
            (1, 1, 9007199254740993, 9007199254740992.0),
            (1, 2, 9007199254740993, 9007199254740992.0),
            (2, 1, 9007199254740992, 9007199254740992.0),
            (2, 2, 9007199254740992, 9007199254740992.0),
            (3, 3, 9007199254740991, 9007199254740991.0),
            (5, 5, -9007199254740993, -9007199254740992.0),
            (6, 6, 9223372036854775807, 9223372036854775808.0),
        ],
        _ => panic!("unknown native family"),
    };
    let mut rows: Vec<_> = tuples
        .into_iter()
        .map(|(l, r, integer, float)| {
            vec![
                Value::Int64(l),
                Value::Int64(r),
                Value::Int64(integer),
                Value::Float64(float),
            ]
        })
        .collect();
    if left {
        rows.push(vec![
            Value::Int64(if family == "int8" { 4 } else { 5 }),
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
    }
    sort_bag(&mut rows);
    rows
}

fn sort_bag(rows: &mut [Vec<Value>]) {
    fn key(row: &[Value]) -> (i64, Option<i64>) {
        let Value::Int64(left) = row[0] else {
            panic!("integer left id")
        };
        let right = match row[1] {
            Value::Int64(value) => Some(value),
            Value::Null => None,
            _ => panic!("integer or null right id"),
        };
        (left, right)
    }
    rows.sort_by_key(|row| key(row));
}

fn assert_completed(observed: &Observation, strategy: &str, family: &str, left: bool) {
    let expected = pg_rows(family, left);
    let matched = if family == "int8" { 7 } else { 6 };
    let mut actual = observed.rows.clone();
    sort_bag(&mut actual);
    assert_eq!(
        actual, expected,
        "literal PostgreSQL bag, including original integer carriers"
    );
    assert_eq!(
        observed.columns,
        ["left_id", "right_id", "integer_value", "float_value"]
    );
    assert_eq!(
        delta(&observed.before, &observed.after, "joins", "executions"),
        1
    );
    assert_eq!(observed.after["joins"]["last_strategy"], strategy);
    assert_eq!(
        delta(
            &observed.before,
            &observed.after,
            "joins",
            "matched_rows_total"
        ),
        matched
    );
    assert_eq!(
        delta(
            &observed.before,
            &observed.after,
            "joins",
            "output_rows_total"
        ),
        u64::try_from(expected.len()).expect("small bag")
    );
    let counter = if strategy == "merge" {
        "merge_joins"
    } else {
        "vectorized_joins"
    };
    assert_eq!(
        delta(&observed.before, &observed.after, "joins", counter),
        1
    );
    if strategy != "merge" {
        for counter in [
            "vectorized_build_rows_total",
            "vectorized_probe_rows_total",
            "vectorized_batches_total",
        ] {
            assert!(delta(&observed.before, &observed.after, "joins", counter) > 0);
        }
        assert_eq!(observed.after["joins"]["last_vectorized_batch_size"], 2);
    }
}

#[test]
fn should_match_loaded_mixed_join_rows() {
    // Arrange
    let fixture = fixture("mixed-loaded", true, false);
    for family in ["int2", "int4", "int8"] {
        for left in [false, true] {
            for reverse in [false, true] {
                // Act
                let observed = execute(&fixture, &query(family, left, reverse, true));

                // Assert
                assert_completed(&observed, "typed_hash", family, left);
            }
        }
    }
}

#[test]
fn should_match_streaming_mixed_join_rows() {
    // Arrange
    let fixture = fixture("mixed-streaming", true, false);
    for reverse in [false, true] {
        // Act
        let observed = execute(&fixture, &query("int8", false, reverse, false));

        // Assert
        assert_completed(&observed, "vectorized", "int8", false);
        assert!(
            delta(
                &observed.before,
                &observed.after,
                "read_paths",
                "collection_scans"
            ) > 0
        );
    }
}

#[test]
fn should_preserve_mixed_join_rows_during_merge_selection() {
    // Arrange
    for switching in [false, true] {
        let fixture = fixture("mixed-merge", switching, switching);
        for reverse in [false, true] {
            // Act
            let observed = execute(&fixture, &query("int8", false, reverse, true));

            // Assert
            assert_completed(&observed, "merge", "int8", false);
            assert_eq!(
                delta(
                    &observed.before,
                    &observed.after,
                    "adaptive_candidates",
                    "operator_switch_successes"
                ),
                u64::from(switching)
            );
            if switching {
                assert_eq!(
                    observed.after["adaptive_candidates"]["last_operator_switch_pair"],
                    "vectorized_join_to_merge_join"
                );
                assert_eq!(
                    observed.after["adaptive_candidates"]["last_operator_switch_reason"],
                    "row_threshold_exceeded"
                );
            }
        }
    }
}

#[test]
fn should_avoid_lossy_mixed_join_index_probes() {
    // Arrange
    let fixture = fixture("mixed-index-overlay", true, false);
    fixture
        .execute("CREATE INDEX mix_exact_key ON mix_int8_l (k)")
        .expect("integer index");
    let before_index = fixture.cassie.metrics();
    let indexed = fixture
        .execute("SELECT id FROM mix_int8_l WHERE k = 9007199254740993")
        .expect("exact integer index positive");
    assert_eq!(indexed.rows(), [vec![Value::Int64(1)]]);
    drop(indexed);
    assert!(
        delta(
            &before_index,
            &fixture.cassie.metrics(),
            "read_paths",
            "index_seek_scans"
        ) > 0
    );
    for overlay in [false, true] {
        if overlay {
            fixture.execute("BEGIN").expect("begin overlay");
            // A real staged write preserves the reference bag but disables unsafe index/stream pruning.
            fixture
                .execute("UPDATE mix_int8_l SET k = k WHERE id = 1")
                .expect("stage original exact key");
        }
        for reverse in [false, true] {
            // Act
            let observed = execute(&fixture, &query("int8", false, reverse, false));

            // Assert
            assert_completed(&observed, "typed_hash", "int8", false);
            assert_eq!(
                delta(
                    &observed.before,
                    &observed.after,
                    "read_paths",
                    "index_seek_scans"
                ),
                0
            );
            assert!(
                delta(
                    &observed.before,
                    &observed.after,
                    "read_paths",
                    "collection_scans"
                ) > 0
            );
        }
        if overlay {
            fixture.execute("ROLLBACK").expect("retire overlay");
        }
    }
}
