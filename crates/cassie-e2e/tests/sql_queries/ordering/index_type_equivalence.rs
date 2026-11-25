//! A scalar index never changes a query's rows or their order: for every indexable
//! type, each filter and ordering shape returns exactly what the same query returns
//! without the index.

use super::support_sql_fixture::{sql_fixture, SqlFixture};

/// Runs every probe shape over `column`, using `probes` as comparison constants.
fn shapes(probes: &[&str]) -> Vec<String> {
    let mut queries = Vec::new();
    for probe in probes {
        for op in ["=", ">", ">=", "<", "<="] {
            queries.push(format!("SELECT id FROM t WHERE v {op} {probe} ORDER BY id"));
        }
    }
    if let [low, high, ..] = probes {
        queries.push(format!(
            "SELECT id FROM t WHERE v BETWEEN {low} AND {high} ORDER BY id"
        ));
        queries.push(format!(
            "SELECT id FROM t WHERE v BETWEEN {high} AND {low} ORDER BY id"
        ));
        queries.push(format!(
            "SELECT id FROM t WHERE v IN ({low}, {high}) ORDER BY id"
        ));
    }
    queries.push("SELECT id FROM t ORDER BY v, id LIMIT 3".to_string());
    queries.push("SELECT id FROM t ORDER BY v DESC, id LIMIT 3".to_string());
    queries.push("SELECT id FROM t ORDER BY v, id".to_string());
    queries.push("SELECT id FROM t ORDER BY v DESC, id DESC".to_string());
    queries
}

fn run_all(fixture: &SqlFixture, queries: &[String]) -> Vec<Vec<i64>> {
    queries
        .iter()
        .map(|sql| {
            fixture
                .rows(sql)
                .iter()
                .map(|row| row[0].as_i64().expect("integer id"))
                .collect()
        })
        .collect()
}

/// Builds the table, records every shape's rows, builds the index, and returns
/// the first query whose result changed.
fn first_difference(
    label: &str,
    prelude: &[&str],
    column_type: &str,
    values: &[&str],
    probes: &[&str],
) -> Option<String> {
    let mut setup: Vec<String> = prelude.iter().map(ToString::to_string).collect();
    setup.push(format!(
        "CREATE TABLE t (id INT PRIMARY KEY, v {column_type})"
    ));
    for (index, value) in values.iter().enumerate() {
        setup.push(format!("INSERT INTO t VALUES ({}, {value})", index + 1));
    }
    let statements: Vec<&str> = setup.iter().map(String::as_str).collect();
    let fixture = sql_fixture(label, &statements);
    let queries = shapes(probes);
    let plain = run_all(&fixture, &queries);
    fixture.execute("CREATE INDEX t_v ON t (v)").expect("index");
    let indexed = run_all(&fixture, &queries);
    queries
        .iter()
        .zip(plain.iter().zip(&indexed))
        .find(|(_, (plain, indexed))| plain != indexed)
        .map(|(sql, (plain, indexed))| format!("{sql}: plain {plain:?} indexed {indexed:?}"))
}

macro_rules! equivalence_test {
    ($name:ident, $prelude:expr, $ty:expr, [$($value:expr),+ $(,)?], [$($probe:expr),+ $(,)?]) => {
        #[test]
        fn $name() {
            // Arrange
            let values = [$($value),+];
            let probes = [$($probe),+];

            // Act
            let difference =
                first_difference(stringify!($name), &$prelude, $ty, &values, &probes);

            // Assert
            assert_eq!(difference, None);
        }
    };
}

equivalence_test!(
    should_return_same_rows_with_index_for_smallint,
    [],
    "SMALLINT",
    ["5", "-3", "5", "100", "0"],
    ["0", "5"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_int,
    [],
    "INT",
    ["10", "-9", "9", "2000000000", "0"],
    ["9", "10", "2.5"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_bigint,
    [],
    "BIGINT",
    ["10", "-9", "9", "9007199254740993", "9007199254740992"],
    ["9007199254740992", "10"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_double_precision,
    [],
    "DOUBLE PRECISION",
    ["2.5", "10", "-1.5", "0", "-0.0", "1e300"],
    ["0", "2.5", "10"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_real,
    [],
    "REAL",
    ["2.5", "10.5", "-1.5", "0", "100", "0.1"],
    ["3", "2.5", "0.1"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_numeric,
    [],
    "NUMERIC",
    [
        "2.5",
        "10",
        "-1.5",
        "1.0",
        "1.00",
        "9007199254740993",
        "9007199254740992.5",
        "0.000001"
    ],
    ["3", "1", "2.5", "10"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_numeric_with_scale,
    [],
    "NUMERIC(10,2)",
    ["2.5", "10", "-1.5", "1.0", "1.00", "0.01"],
    ["3", "1", "2.5"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_boolean,
    [],
    "BOOLEAN",
    ["true", "false", "true", "false"],
    ["true", "false"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_text,
    [],
    "TEXT",
    ["'b'", "'a'", "'B'", "'ab'", "'10'", "'9'"],
    ["'a'", "'b'", "'9'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_char_padding,
    [],
    "CHAR(5)",
    ["'ab'", "'abc'", "'b'", "'ab   '", "'a'"],
    ["'ab'", "'b'", "'abc  '"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_varchar,
    [],
    "VARCHAR(8)",
    ["'ab'", "'abc'", "'b'", "'a'"],
    ["'ab'", "'b'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_uuid,
    [],
    "UUID",
    [
        "'00000000-0000-0000-0000-000000000002'",
        "'00000000-0000-0000-0000-000000000001'",
        "'ffffffff-ffff-ffff-ffff-ffffffffffff'",
        "'00000000-0000-0000-0000-00000000000A'"
    ],
    [
        "'00000000-0000-0000-0000-000000000002'",
        "'00000000-0000-0000-0000-00000000000a'"
    ]
);
equivalence_test!(
    should_return_same_rows_with_index_for_bytea,
    [],
    "BYTEA",
    [
        "'\\x01'",
        "'\\x0100'",
        "'\\xff'",
        "'\\x'",
        "'\\x00'",
        "'\\x02'"
    ],
    ["'\\x01'", "'\\x0100'", "'\\xff'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_date,
    [],
    "DATE",
    [
        "'2024-03-01'",
        "'2023-12-31'",
        "'2024-03-01'",
        "'2025-01-01'",
        "'0099-01-01'"
    ],
    ["'2024-03-01'", "'2024-01-01'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_time,
    [],
    "TIME",
    [
        "'10:00'",
        "'09:30:15'",
        "'23:59:59.5'",
        "'00:00'",
        "'10:00:00'"
    ],
    ["'10:00'", "'09:30:15'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_timestamp,
    [],
    "TIMESTAMP",
    [
        "'2024-01-01 10:00:00'",
        "'2024-01-01 09:00:00.5'",
        "'2023-06-01 00:00:00'",
        "'2024-01-01 10:00:00.000'"
    ],
    ["'2024-01-01 10:00:00'", "'2023-06-01 00:00:00'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_timestamptz,
    [],
    "TIMESTAMPTZ",
    [
        "'2024-01-01 10:00:00+00'",
        "'2024-01-01 12:00:00+05'",
        "'2023-06-01 00:00:00-08'",
        "'2024-01-01 02:00:00-08'"
    ],
    ["'2024-01-01 10:00:00+00'", "'2023-06-01 08:00:00+00'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_interval,
    [],
    "INTERVAL",
    [
        "'1 day'",
        "'25 hours'",
        "'2 days'",
        "'1 mon'",
        "'30 days'",
        "'24 hours'"
    ],
    ["'1 day'", "'30 days'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_inet,
    [],
    "INET",
    [
        "'10.0.0.1'",
        "'9.0.0.1'",
        "'192.168.0.1/24'",
        "'10.0.0.1/32'"
    ],
    ["'10.0.0.1'::inet", "'9.0.0.1'::inet"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_enum,
    ["CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')"],
    "mood",
    ["'happy'", "'sad'", "'ok'", "'sad'"],
    ["'ok'", "'sad'"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_domain_over_numeric,
    ["CREATE DOMAIN money_amount AS NUMERIC"],
    "money_amount",
    ["2.5", "10", "-1.5", "1.0", "1.00"],
    ["3", "1"]
);
equivalence_test!(
    should_return_same_rows_with_index_for_domain_over_int,
    ["CREATE DOMAIN small_count AS INT"],
    "small_count",
    ["10", "-9", "9", "10"],
    ["9", "10"]
);
