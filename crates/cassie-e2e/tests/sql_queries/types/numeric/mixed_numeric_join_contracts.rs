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
            (3, 3, 32_767, 32_767.0),
            (4, 4, -32_768, -32_768.0),
        ],
        "int4" => vec![
            (1, 1, 16_777_217, 16_777_217.0),
            (1, 2, 16_777_217, 16_777_217.0),
            (2, 1, 16_777_217, 16_777_217.0),
            (2, 2, 16_777_217, 16_777_217.0),
            (3, 3, 2_147_483_647, 2_147_483_647.0),
            (4, 4, -2_147_483_648, -2_147_483_648.0),
        ],
        "int8" => vec![
            (1, 1, 9_007_199_254_740_993, 9_007_199_254_740_992.0),
            (1, 2, 9_007_199_254_740_993, 9_007_199_254_740_992.0),
            (2, 1, 9_007_199_254_740_992, 9_007_199_254_740_992.0),
            (2, 2, 9_007_199_254_740_992, 9_007_199_254_740_992.0),
            (3, 3, 9_007_199_254_740_991, 9_007_199_254_740_991.0),
            (5, 5, -9_007_199_254_740_993, -9_007_199_254_740_992.0),
            (6, 6, 9_223_372_036_854_775_807, 9_223_372_036_854_775_808.0),
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

include!("mixed_numeric_joins.rs");

mod scalar_index_probes {
    use super::{delta, sql_fixture_with_config};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::types::Value;

    const SEED: &[&str] = &[
        "CREATE TABLE mix_scalar_index (id INT,k BIGINT)",
        "INSERT INTO mix_scalar_index(id,k) VALUES (1,9007199254740991),(2,9007199254740992),(3,9007199254740993),(4,9007199254740993),(5,NULL),(6,-9007199254740993),(7,-9007199254740992)",
        "CREATE INDEX mix_scalar_exact_key ON mix_scalar_index(k)",
    ];

    fn accounted_memory(metrics: &serde_json::Value) -> u64 {
        metrics["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("query owner metric")
    }

    fn expected_rows(overlay: bool, family: usize) -> Vec<Vec<Value>> {
        match family {
            0 => {
                let mut rows = vec![
                    vec![Value::Int64(2), Value::Int64(9_007_199_254_740_992)],
                    vec![Value::Int64(3), Value::Int64(9_007_199_254_740_993)],
                    vec![Value::Int64(4), Value::Int64(9_007_199_254_740_993)],
                ];
                if overlay {
                    rows.insert(
                        0,
                        vec![Value::Int64(1), Value::Int64(9_007_199_254_740_993)],
                    );
                }
                rows
            }
            1 => vec![
                vec![Value::Int64(6), Value::Int64(-9_007_199_254_740_993)],
                vec![Value::Int64(7), Value::Int64(-9_007_199_254_740_992)],
            ],
            2 => vec![vec![Value::Int64(5), Value::Null]],
            3 => vec![
                vec![Value::Int64(3), Value::Int64(9_007_199_254_740_993)],
                vec![Value::Int64(4), Value::Int64(9_007_199_254_740_993)],
            ],
            _ => panic!("literal observed family"),
        }
    }

    const FLOAT_INDEX_SEED: &[&str] = &[
        "CREATE TABLE mixed_float_scan (id INT,x FLOAT)",
        "CREATE TABLE mixed_float_indexed (id INT,x FLOAT)",
        "INSERT INTO mixed_float_scan(id,x) VALUES (1,9007199254740991.0),(2,9007199254740992.0),(3,9007199254740992.0),(4,9007199254740994.0),(5,-9007199254740994.0),(6,-9007199254740992.0),(7,-9007199254740992.0),(8,-9007199254740991.0),(9,NULL),(10,-0.0),(11,0.0)",
        "INSERT INTO mixed_float_indexed(id,x) VALUES (1,9007199254740991.0),(2,9007199254740992.0),(3,9007199254740992.0),(4,9007199254740994.0),(5,-9007199254740994.0),(6,-9007199254740992.0),(7,-9007199254740992.0),(8,-9007199254740991.0),(9,NULL),(10,-0.0),(11,0.0)",
        "CREATE INDEX mixed_float_key ON mixed_float_indexed(x)",
    ];

    // Literal bags after the selected IntegerLiteral-to-FLOAT coercion. No
    // floating-point comparison in this test computes an expected row.
    const FLOAT_INDEX_CASES: &[(&str, &[i64], &str)] = &[
        ("x = 9007199254740993", &[2, 3], "index_seek_scans"),
        ("9007199254740993 = x", &[2, 3], "index_seek_scans"),
        (
            "x < 9007199254740993",
            &[1, 5, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        (
            "9007199254740993 > x",
            &[1, 5, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        (
            "x <= 9007199254740993",
            &[1, 2, 3, 5, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        (
            "9007199254740993 >= x",
            &[1, 2, 3, 5, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        ("x > 9007199254740993", &[4], "range_scans"),
        ("9007199254740993 < x", &[4], "range_scans"),
        ("x >= 9007199254740993", &[2, 3, 4], "range_scans"),
        ("9007199254740993 <= x", &[2, 3, 4], "range_scans"),
        ("x = -9007199254740993", &[6, 7], "index_seek_scans"),
        ("-9007199254740993 = x", &[6, 7], "index_seek_scans"),
        ("x < -9007199254740993", &[5], "range_scans"),
        ("-9007199254740993 > x", &[5], "range_scans"),
        ("x <= -9007199254740993", &[5, 6, 7], "range_scans"),
        ("-9007199254740993 >= x", &[5, 6, 7], "range_scans"),
        (
            "x > -9007199254740993",
            &[1, 2, 3, 4, 8, 10, 11],
            "range_scans",
        ),
        (
            "-9007199254740993 < x",
            &[1, 2, 3, 4, 8, 10, 11],
            "range_scans",
        ),
        (
            "x >= -9007199254740993",
            &[1, 2, 3, 4, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        (
            "-9007199254740993 <= x",
            &[1, 2, 3, 4, 6, 7, 8, 10, 11],
            "range_scans",
        ),
        ("x = 0", &[10, 11], "collection_scans"),
        ("0 = x", &[10, 11], "collection_scans"),
        ("x < 0", &[5, 6, 7, 8], "collection_scans"),
        ("0 > x", &[5, 6, 7, 8], "collection_scans"),
        ("x <= 0", &[5, 6, 7, 8, 10, 11], "collection_scans"),
        ("0 >= x", &[5, 6, 7, 8, 10, 11], "collection_scans"),
        ("x > 0", &[1, 2, 3, 4], "collection_scans"),
        ("0 < x", &[1, 2, 3, 4], "collection_scans"),
        ("x >= 0", &[1, 2, 3, 4, 10, 11], "collection_scans"),
        ("0 <= x", &[1, 2, 3, 4, 10, 11], "collection_scans"),
    ];

    const QUOTED_FLOAT_SEED: &[&str] = &[
        "CREATE TABLE quoted_float_scan (id INT,\"Float.Key\" FLOAT,tag INT)",
        "CREATE TABLE quoted_float_indexed (id INT,\"Float.Key\" FLOAT,tag INT)",
        "INSERT INTO quoted_float_scan VALUES (1,9007199254740991.0,7),(2,9007199254740992.0,7),(3,9007199254740992.0,8),(4,9007199254740992.0,NULL),(5,9007199254740994.0,7),(6,-9007199254740994.0,7),(7,-9007199254740992.0,7),(8,-9007199254740992.0,8),(9,-9007199254740992.0,NULL),(10,-9007199254740991.0,7),(11,NULL,7)",
        "INSERT INTO quoted_float_indexed VALUES (1,9007199254740991.0,7),(2,9007199254740992.0,7),(3,9007199254740992.0,8),(4,9007199254740992.0,NULL),(5,9007199254740994.0,7),(6,-9007199254740994.0,7),(7,-9007199254740992.0,7),(8,-9007199254740992.0,8),(9,-9007199254740992.0,NULL),(10,-9007199254740991.0,7),(11,NULL,7)",
        "CREATE INDEX quoted_float_key ON quoted_float_indexed(\"Float.Key\",tag)",
    ];

    const QUOTED_FLOAT_CASES: &[(&str, bool, &[i64], &str)] = &[
        (
            "\"Float.Key\" = 9007199254740993 AND tag = 7",
            false,
            &[2],
            "prefix",
        ),
        (
            "f.tag = 7 AND 9007199254740993 = f.\"Float.Key\"",
            true,
            &[2],
            "prefix",
        ),
        (
            "f.\"Float.Key\" = -9007199254740993 AND f.tag = 7",
            true,
            &[7],
            "prefix",
        ),
        (
            "\"Float.Key\" >= 9007199254740993 AND tag = 7",
            false,
            &[2, 5],
            "range",
        ),
        (
            "7 = f.tag AND -9007199254740993 <= f.\"Float.Key\"",
            true,
            &[1, 2, 5, 7, 10],
            "range",
        ),
        (
            "\"Float.Key\" = 9007199254740993",
            false,
            &[2, 3, 4],
            "scan",
        ),
    ];

    struct FloatIndexObservation {
        rows: Result<(Vec<Vec<Value>>, u64), cassie::app::CassieError>,
        seeks: u64,
        prefixes: u64,
        ranges: u64,
        scans: u64,
        retired: u64,
    }

    fn observe_float_index_read(
        fixture: &super::SqlFixture,
        table: &str,
        predicate: &str,
    ) -> FloatIndexObservation {
        let before = fixture.cassie.metrics();
        let sql = format!("SELECT id FROM {table} WHERE {predicate}");
        let rows = fixture.execute(&sql).map(|result| {
            let rows = result.rows().to_vec();
            let live = accounted_memory(&fixture.cassie.metrics());
            drop(result);
            (rows, live)
        });
        let after = fixture.cassie.metrics();
        FloatIndexObservation {
            rows,
            seeks: delta(&before, &after, "read_paths", "index_seek_scans"),
            prefixes: delta(&before, &after, "read_paths", "prefix_scans"),
            ranges: delta(&before, &after, "read_paths", "range_scans"),
            scans: delta(&before, &after, "read_paths", "collection_scans"),
            retired: accounted_memory(&after),
        }
    }

    include!("scalar_index_probes.rs");
}
