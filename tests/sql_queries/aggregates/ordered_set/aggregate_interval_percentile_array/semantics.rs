//! PostgreSQL 18.6 interval ordered-set percentile array contract for #806.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_interpolate_multiple_continuous_percentiles_for_interval_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-interval-percentile-array",
        &[
            "CREATE TABLE issue_806_interval_percentiles (duration INTERVAL)",
            "INSERT INTO issue_806_interval_percentiles VALUES (INTERVAL '0 hours'), (INTERVAL '4 hours'), (INTERVAL '8 hours'), (INTERVAL '12 hours'), (NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_cont(ARRAY[0.25,0.5,0.75]) WITHIN GROUP (ORDER BY duration)::TEXT, \
                pg_typeof(percentile_cont(ARRAY[0.25,0.5,0.75]) WITHIN GROUP (ORDER BY duration))::OID \
         FROM issue_806_interval_percentiles",
    );

    // Assert
    let result = result.expect("array fractions interpolate ordered interval values");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{03:00:00,06:00:00,09:00:00}".to_owned()),
            Value::Int64(1187),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 26]
    );
}
