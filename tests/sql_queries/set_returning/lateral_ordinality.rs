//! Correlated lateral series and WITH ORDINALITY under a result row limit (#808).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_ordinality_given_a_correlated_lateral_function_when_a_row_limit_is_applied() {
    // Arrange
    let fixture = sql_fixture(
        "lateral-series-ordinality-row-limit",
        &[
            "CREATE TABLE issue_808_lateral_sources (id INT4 PRIMARY KEY, stop_at INT4)",
            "INSERT INTO issue_808_lateral_sources VALUES (1, 3), (2, 2)",
        ],
    );
    let control = fixture.execute(
        "SELECT source.id, generated.value, generated.ordinality \
         FROM issue_808_lateral_sources AS source \
         CROSS JOIN LATERAL generate_series(1, source.stop_at) \
             WITH ORDINALITY AS generated(value, ordinality) \
         ORDER BY source.id, generated.ordinality",
    );
    let limited = fixture.execute(
        "SELECT source.id, generated.value, generated.ordinality \
         FROM issue_808_lateral_sources AS source \
         CROSS JOIN LATERAL generate_series(1, source.stop_at) \
             WITH ORDINALITY AS generated(value, ordinality) \
         ORDER BY source.id, generated.ordinality LIMIT 4",
    );

    // Act
    let control = control.expect("unlimited correlated lateral control");
    let limited = limited.expect("limited correlated lateral series");

    // Assert
    assert_eq!(
        control.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(3), Value::Int64(3)],
            vec![Value::Int64(2), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2), Value::Int64(2)],
        ]
    );
    assert_eq!(
        limited
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23, 20]
    );
    assert_eq!(
        limited.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(3), Value::Int64(3)],
            vec![Value::Int64(2), Value::Int64(1), Value::Int64(1)],
        ]
    );
}
