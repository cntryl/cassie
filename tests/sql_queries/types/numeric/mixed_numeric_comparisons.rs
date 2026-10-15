//! Literal oracles retained from PostgreSQL 18.6, p904num01 at 9a26dfa1.
use super::support_mixed_numeric::{boolean_row, projection};
use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const EQUAL: [Option<bool>; 8] = [
    Some(true),
    Some(false),
    Some(false),
    Some(true),
    Some(false),
    Some(true),
    Some(true),
    Some(false),
];

#[test]
fn should_resolve_mixed_scalar_comparison_domains() {
    // Arrange
    let fixture = sql_fixture("mixed-numeric-scalars", &[]);
    let pairs = [
        ("CAST('7' AS SMALLINT)", "CAST('7' AS FLOAT)"),
        ("CAST('2147483647' AS INT)", "CAST('2147483647' AS FLOAT)"),
        (
            "CAST('9007199254740993' AS BIGINT)",
            "CAST('9007199254740992' AS FLOAT)",
        ),
        (
            "CAST('-9007199254740993' AS BIGINT)",
            "CAST('-9007199254740992' AS FLOAT)",
        ),
        (
            "CAST('9223372036854775807' AS BIGINT)",
            "CAST('9223372036854775808' AS FLOAT)",
        ),
        (
            "CAST('-9223372036854775808' AS BIGINT)",
            "CAST('-9223372036854775808' AS FLOAT)",
        ),
        ("CAST('0' AS BIGINT)", "CAST('-0.0' AS FLOAT)"),
    ];
    let queries = pairs.into_iter().flat_map(|(left, right)| {
        [(left, right), (right, left)]
            .map(|(left, right)| format!("SELECT {}", projection(left, right)))
    });

    // Act
    let results = queries
        .map(|sql| {
            (
                sql.clone(),
                fixture.execute(&sql).expect("primitive comparison"),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    for (sql, result) in results {
        assert_eq!(result.rows(), &[boolean_row(EQUAL)], "{sql}");
        assert_eq!(result.command(), "SELECT");
        assert_eq!(result.columns().len(), 8);
        for column in result.columns() {
            assert_eq!(
                (column.type_oid, column.typlen, column.atttypmod),
                (16, 1, -1)
            );
        }
    }
}

#[test]
fn should_preserve_numeric_comparison_boundaries() {
    // Arrange
    let fixture = sql_fixture("mixed-numeric-boundaries", &[]);
    let greater = [
        Some(false),
        Some(true),
        Some(false),
        Some(false),
        Some(true),
        Some(true),
        Some(false),
        Some(true),
    ];
    let one_null = [None, None, None, None, None, None, Some(false), Some(true)];
    let two_nulls = [None, None, None, None, None, None, Some(true), Some(false)];
    let cases = [
        (
            "CAST('9007199254740993' AS BIGINT)",
            "CAST('9007199254740992' AS BIGINT)",
            greater,
        ),
        (
            "CAST('9007199254740993' AS FLOAT)",
            "CAST('9007199254740992' AS FLOAT)",
            EQUAL,
        ),
        ("CAST(NULL AS BIGINT)", "CAST('7' AS FLOAT)", one_null),
        ("CAST('7' AS BIGINT)", "CAST(NULL AS FLOAT)", one_null),
        ("CAST(NULL AS BIGINT)", "CAST(NULL AS FLOAT)", two_nulls),
    ];

    // Act
    let results = cases.map(|(left, right, expected)| {
        let sql = format!("SELECT {}", projection(left, right));
        (fixture.rows(&sql), expected)
    });
    let carrier = fixture.rows("SELECT CAST('9007199254740993' AS BIGINT) AS exact_integer, CAST('9007199254740993' AS FLOAT) AS rounded_float");
    let deferred = fixture.rows("SELECT CAST('9007199254740993' AS BIGINT) IN (CAST('9007199254740992' AS FLOAT)) AS membership, CAST('9007199254740993' AS BIGINT) BETWEEN CAST('9007199254740992' AS FLOAT) AND CAST('9007199254740992' AS FLOAT) AS bounded, CASE CAST('9007199254740993' AS BIGINT) WHEN CAST('9007199254740992' AS FLOAT) THEN TRUE ELSE FALSE END AS chosen");
    let numeric_literals = fixture.rows("SELECT 9007199254740993 IS DISTINCT FROM 9007199254740992.0 AS positive, -9007199254740993 IS DISTINCT FROM -9007199254740992.0 AS negative, 9007199254740993 IS DISTINCT FROM (9007199254740992.0 + 0) AS arithmetic");

    // Assert
    for (rows, expected) in results {
        assert_eq!(rows, vec![boolean_row(expected)]);
    }
    assert_eq!(
        carrier,
        vec![vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Float64(9_007_199_254_740_992.0)
        ]]
    );
    // The separate IN/BETWEEN/simple CASE owners retain their current law.
    assert_eq!(deferred, vec![vec![Value::Bool(false); 3]]);
    assert_eq!(numeric_literals, vec![vec![Value::Bool(true); 3]]);
}

#[test]
fn should_preserve_numeric_expression_origins() {
    // Arrange
    let fixture = sql_fixture("numeric-expression-origins", &[]);
    let cases = NUMERIC_EXPRESSION_ORIGINS;
    let names = ["eq", "ne", "lt", "le", "gt", "ge", "same", "different"];

    // Act
    let observations = cases
        .iter()
        .map(|(sql, expected)| {
            let observed = fixture.execute(sql).map(|result| {
                let columns = result
                    .columns()
                    .iter()
                    .map(|column| {
                        (
                            column.name.clone(),
                            column.type_oid,
                            column.typlen,
                            column.atttypmod,
                        )
                    })
                    .collect::<Vec<_>>();
                (
                    result.rows().to_vec(),
                    columns,
                    result.command().to_string(),
                )
            });
            (*sql, *expected, observed)
        })
        .collect::<Vec<_>>();
    drop(fixture);

    // Assert
    for (sql, expected, observed) in observations {
        let (rows, columns, command) = observed.expect("native-observed expression is admitted");
        assert_eq!(rows, vec![boolean_row(expected)], "{sql}");
        assert_eq!(command, "SELECT", "{sql}");
        assert_eq!(columns.len(), names.len(), "{sql}");
        for (column, name) in columns.iter().zip(names) {
            assert_eq!(column, &(name.to_string(), 16, 1, -1), "{sql}");
        }
    }
}

#[test]
fn should_evaluate_derived_numeric_comparisons() {
    // Arrange
    let fixture = sql_fixture("derived-numeric-comparisons", &[]);
    // Exact PostgreSQL 18.6 p904numprov01 SQL and literal Boolean rows.
    let cases = [
        (
            "SELECT CAST(9007199254740993 AS BIGINT) = (decimal_source.n) AS eq, CAST(9007199254740993 AS BIGINT) <> (decimal_source.n) AS ne, CAST(9007199254740993 AS BIGINT) < (decimal_source.n) AS lt, CAST(9007199254740993 AS BIGINT) <= (decimal_source.n) AS le, CAST(9007199254740993 AS BIGINT) > (decimal_source.n) AS gt, CAST(9007199254740993 AS BIGINT) >= (decimal_source.n) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (decimal_source.n) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (decimal_source.n) AS different FROM (SELECT 9007199254740992.0 AS n) AS decimal_source",
            [
                Some(false),
                Some(true),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
                Some(true),
            ],
        ),
        (
            "SELECT CAST(9007199254740993 AS BIGINT) = (float_source.n) AS eq, CAST(9007199254740993 AS BIGINT) <> (float_source.n) AS ne, CAST(9007199254740993 AS BIGINT) < (float_source.n) AS lt, CAST(9007199254740993 AS BIGINT) <= (float_source.n) AS le, CAST(9007199254740993 AS BIGINT) > (float_source.n) AS gt, CAST(9007199254740993 AS BIGINT) >= (float_source.n) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (float_source.n) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (float_source.n) AS different FROM (SELECT CAST(9007199254740992.0 AS FLOAT) AS n) AS float_source",
            [
                Some(true),
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
            ],
        ),
    ];
    let names = ["eq", "ne", "lt", "le", "gt", "ge", "same", "different"];

    // Act
    let observations = cases.map(|(sql, expected)| {
        let observed = fixture.execute(sql).map(|result| {
            let columns = result
                .columns()
                .iter()
                .map(|column| {
                    (
                        column.name.clone(),
                        column.type_oid,
                        column.typlen,
                        column.atttypmod,
                    )
                })
                .collect::<Vec<_>>();
            (
                result.rows().to_vec(),
                columns,
                result.command().to_string(),
            )
        });
        (sql, expected, observed)
    });
    drop(fixture);

    // Assert
    for (sql, expected, observed) in observations {
        let (rows, columns, command) = observed.expect("derived comparison is admitted");
        assert_eq!(rows, vec![boolean_row(expected)], "{sql}");
        assert_eq!(command, "SELECT", "{sql}");
        assert_eq!(columns.len(), names.len(), "{sql}");
        for (column, name) in columns.iter().zip(names) {
            assert_eq!(column, &(name.to_string(), 16, 1, -1), "{sql}");
        }
    }
}

// Literal PostgreSQL 18.6 vectors retained from p904numprov01.
const NUMERIC_ORIGIN_PATTERN_0: [Option<bool>; 8] = [
    Some(false),
    Some(true),
    Some(false),
    Some(false),
    Some(true),
    Some(true),
    Some(false),
    Some(true),
];
const NUMERIC_ORIGIN_PATTERN_1: [Option<bool>; 8] = [
    Some(true),
    Some(false),
    Some(false),
    Some(true),
    Some(false),
    Some(true),
    Some(true),
    Some(false),
];
const NUMERIC_ORIGIN_PATTERN_2: [Option<bool>; 8] = [
    Some(false),
    Some(true),
    Some(true),
    Some(true),
    Some(false),
    Some(false),
    Some(false),
    Some(true),
];
const NUMERIC_EXPRESSION_ORIGINS: &[(&str, [Option<bool>; 8])] = &[
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (9007199254740992.0) AS eq, CAST(9007199254740993 AS BIGINT) <> (9007199254740992.0) AS ne, CAST(9007199254740993 AS BIGINT) < (9007199254740992.0) AS lt, CAST(9007199254740993 AS BIGINT) <= (9007199254740992.0) AS le, CAST(9007199254740993 AS BIGINT) > (9007199254740992.0) AS gt, CAST(9007199254740993 AS BIGINT) >= (9007199254740992.0) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (9007199254740992.0) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (9007199254740992.0) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((9007199254740992.0) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((9007199254740992.0) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((9007199254740992.0) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((9007199254740992.0) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((9007199254740992.0) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((9007199254740992.0) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((9007199254740992.0) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((9007199254740992.0) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(-9007199254740993 AS BIGINT) = (-9007199254740992.0) AS eq, CAST(-9007199254740993 AS BIGINT) <> (-9007199254740992.0) AS ne, CAST(-9007199254740993 AS BIGINT) < (-9007199254740992.0) AS lt, CAST(-9007199254740993 AS BIGINT) <= (-9007199254740992.0) AS le, CAST(-9007199254740993 AS BIGINT) > (-9007199254740992.0) AS gt, CAST(-9007199254740993 AS BIGINT) >= (-9007199254740992.0) AS ge, CAST(-9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (-9007199254740992.0) AS same, CAST(-9007199254740993 AS BIGINT) IS DISTINCT FROM (-9007199254740992.0) AS different",
        NUMERIC_ORIGIN_PATTERN_2,
    ),
    (
        "SELECT CAST(-9007199254740993 AS BIGINT) = (CAST((-9007199254740992.0) AS FLOAT)) AS eq, CAST(-9007199254740993 AS BIGINT) <> (CAST((-9007199254740992.0) AS FLOAT)) AS ne, CAST(-9007199254740993 AS BIGINT) < (CAST((-9007199254740992.0) AS FLOAT)) AS lt, CAST(-9007199254740993 AS BIGINT) <= (CAST((-9007199254740992.0) AS FLOAT)) AS le, CAST(-9007199254740993 AS BIGINT) > (CAST((-9007199254740992.0) AS FLOAT)) AS gt, CAST(-9007199254740993 AS BIGINT) >= (CAST((-9007199254740992.0) AS FLOAT)) AS ge, CAST(-9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((-9007199254740992.0) AS FLOAT)) AS same, CAST(-9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((-9007199254740992.0) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = ((9007199254740992.0 + 0)) AS eq, CAST(9007199254740993 AS BIGINT) <> ((9007199254740992.0 + 0)) AS ne, CAST(9007199254740993 AS BIGINT) < ((9007199254740992.0 + 0)) AS lt, CAST(9007199254740993 AS BIGINT) <= ((9007199254740992.0 + 0)) AS le, CAST(9007199254740993 AS BIGINT) > ((9007199254740992.0 + 0)) AS gt, CAST(9007199254740993 AS BIGINT) >= ((9007199254740992.0 + 0)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM ((9007199254740992.0 + 0)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM ((9007199254740992.0 + 0)) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST(((9007199254740992.0 + 0)) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (COALESCE(9007199254740992.0, NULL)) AS eq, CAST(9007199254740993 AS BIGINT) <> (COALESCE(9007199254740992.0, NULL)) AS ne, CAST(9007199254740993 AS BIGINT) < (COALESCE(9007199254740992.0, NULL)) AS lt, CAST(9007199254740993 AS BIGINT) <= (COALESCE(9007199254740992.0, NULL)) AS le, CAST(9007199254740993 AS BIGINT) > (COALESCE(9007199254740992.0, NULL)) AS gt, CAST(9007199254740993 AS BIGINT) >= (COALESCE(9007199254740992.0, NULL)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (COALESCE(9007199254740992.0, NULL)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (COALESCE(9007199254740992.0, NULL)) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((COALESCE(9007199254740992.0, NULL)) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS eq, CAST(9007199254740993 AS BIGINT) <> (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS ne, CAST(9007199254740993 AS BIGINT) < (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS lt, CAST(9007199254740993 AS BIGINT) <= (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS le, CAST(9007199254740993 AS BIGINT) > (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS gt, CAST(9007199254740993 AS BIGINT) >= (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE NULL END) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS eq, CAST(9007199254740993 AS BIGINT) <> (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS ne, CAST(9007199254740993 AS BIGINT) < (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS lt, CAST(9007199254740993 AS BIGINT) <= (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS le, CAST(9007199254740993 AS BIGINT) > (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS gt, CAST(9007199254740993 AS BIGINT) >= (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((CASE WHEN TRUE THEN 9007199254740992.0 ELSE 1 END) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS eq, CAST(9007199254740993 AS BIGINT) <> (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS ne, CAST(9007199254740993 AS BIGINT) < (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS lt, CAST(9007199254740993 AS BIGINT) <= (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS le, CAST(9007199254740993 AS BIGINT) > (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS gt, CAST(9007199254740993 AS BIGINT) >= (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS different",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((COALESCE(NULLIF(9007199254740992.0, 0), NULL)) AS FLOAT)) AS different",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "SELECT CAST(9007199254740993 AS BIGINT) = (CAST((decimal_source.n) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((decimal_source.n) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((decimal_source.n) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((decimal_source.n) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((decimal_source.n) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((decimal_source.n) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((decimal_source.n) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((decimal_source.n) AS FLOAT)) AS different FROM (SELECT 9007199254740992.0 AS n) AS decimal_source",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "WITH decimal_source AS (SELECT 9007199254740992.0 AS n) SELECT CAST(9007199254740993 AS BIGINT) = (decimal_source.n) AS eq, CAST(9007199254740993 AS BIGINT) <> (decimal_source.n) AS ne, CAST(9007199254740993 AS BIGINT) < (decimal_source.n) AS lt, CAST(9007199254740993 AS BIGINT) <= (decimal_source.n) AS le, CAST(9007199254740993 AS BIGINT) > (decimal_source.n) AS gt, CAST(9007199254740993 AS BIGINT) >= (decimal_source.n) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (decimal_source.n) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (decimal_source.n) AS different FROM decimal_source",
        NUMERIC_ORIGIN_PATTERN_0,
    ),
    (
        "WITH decimal_source AS (SELECT 9007199254740992.0 AS n) SELECT CAST(9007199254740993 AS BIGINT) = (CAST((decimal_source.n) AS FLOAT)) AS eq, CAST(9007199254740993 AS BIGINT) <> (CAST((decimal_source.n) AS FLOAT)) AS ne, CAST(9007199254740993 AS BIGINT) < (CAST((decimal_source.n) AS FLOAT)) AS lt, CAST(9007199254740993 AS BIGINT) <= (CAST((decimal_source.n) AS FLOAT)) AS le, CAST(9007199254740993 AS BIGINT) > (CAST((decimal_source.n) AS FLOAT)) AS gt, CAST(9007199254740993 AS BIGINT) >= (CAST((decimal_source.n) AS FLOAT)) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (CAST((decimal_source.n) AS FLOAT)) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (CAST((decimal_source.n) AS FLOAT)) AS different FROM decimal_source",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
    (
        "WITH float_source AS (SELECT CAST(9007199254740992.0 AS FLOAT) AS n) SELECT CAST(9007199254740993 AS BIGINT) = (float_source.n) AS eq, CAST(9007199254740993 AS BIGINT) <> (float_source.n) AS ne, CAST(9007199254740993 AS BIGINT) < (float_source.n) AS lt, CAST(9007199254740993 AS BIGINT) <= (float_source.n) AS le, CAST(9007199254740993 AS BIGINT) > (float_source.n) AS gt, CAST(9007199254740993 AS BIGINT) >= (float_source.n) AS ge, CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM (float_source.n) AS same, CAST(9007199254740993 AS BIGINT) IS DISTINCT FROM (float_source.n) AS different FROM float_source",
        NUMERIC_ORIGIN_PATTERN_1,
    ),
];

#[test]
fn should_preserve_float_origins_in_nested_cte_predicates() {
    // Arrange
    let fixture = sql_fixture("nested-cte-float-origins", &[]);
    let cases = [
        (
            "WITH f AS (SELECT CAST(9007199254740992.0 AS FLOAT) AS n) SELECT EXISTS(SELECT 1 FROM f WHERE CAST(9007199254740993 AS BIGINT) = f.n) AS matched",
            true,
        ),
        (
            "WITH f AS (SELECT 9007199254740992.0 AS n) SELECT EXISTS(SELECT 1 FROM f WHERE CAST(9007199254740993 AS BIGINT) = f.n) AS matched",
            false,
        ),
    ];

    // Act
    let observations = cases.map(|(sql, expected)| {
        let rows = fixture.execute(sql).map(|result| result.rows().to_vec());
        (sql, expected, rows)
    });
    drop(fixture);

    // Assert
    for (sql, expected, rows) in observations {
        assert_eq!(
            rows.expect("nested CTE predicate"),
            vec![vec![Value::Bool(expected)]],
            "{sql}"
        );
    }
}
