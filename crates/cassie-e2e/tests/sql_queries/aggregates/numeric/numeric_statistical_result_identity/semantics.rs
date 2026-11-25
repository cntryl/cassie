//! PostgreSQL 18.6 numeric result identity for integer statistical aggregates (#806).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_numeric_result_identity_for_integer_statistical_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-integer-statistics-numeric-result-identity",
        &[
            "CREATE TABLE issue_806_integer_statistics (value INTEGER)",
            "INSERT INTO issue_806_integer_statistics VALUES (1),(2),(4),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT var_pop(value), var_samp(value), variance(value), \
         stddev_pop(value), stddev_samp(value), stddev(value), \
         var_pop(value)::TEXT, var_samp(value)::TEXT, variance(value)::TEXT, \
         stddev_pop(value)::TEXT, stddev_samp(value)::TEXT, stddev(value)::TEXT \
         FROM issue_806_integer_statistics",
    );

    // Assert
    let result = result.expect("integer statistical aggregates return PostgreSQL numeric values");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700, 1700, 1700, 1700, 1700, 25, 25, 25, 25, 25, 25]
    );
    assert_eq!(
        &result.rows()[0][6..],
        &[
            Value::String("1.5555555555555556".to_owned()),
            Value::String("2.3333333333333333".to_owned()),
            Value::String("2.3333333333333333".to_owned()),
            Value::String("1.2472191289246471".to_owned()),
            Value::String("1.5275252316519467".to_owned()),
            Value::String("1.5275252316519467".to_owned()),
        ]
    );
}
