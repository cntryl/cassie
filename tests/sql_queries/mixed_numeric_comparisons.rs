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
fn should_preserve_numeric_nulls_and_exact_integer_boundaries() {
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
}
