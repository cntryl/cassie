//! Element-wise conversion when an array value is cast to another array type.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_cast_numeric_array_constructors_to_other_numeric_array_types() {
    // Arrange
    let fixture = sql_fixture("array-element-casts", &[]);
    let cases = [
        ("SELECT (ARRAY[1.0, 2.0, 3.0]::real[])::TEXT", "{1,2,3}"),
        ("SELECT (ARRAY[1.5, 2.5]::float8[])::TEXT", "{1.5,2.5}"),
        ("SELECT (ARRAY[1.0, 2.0]::int[])::TEXT", "{1,2}"),
        ("SELECT (ARRAY[1.5, 2.5]::int[])::TEXT", "{2,3}"),
        ("SELECT (ARRAY[1.0, 2.0]::bigint[])::TEXT", "{1,2}"),
        ("SELECT (ARRAY[1, 2]::real[])::TEXT", "{1,2}"),
        ("SELECT (ARRAY[1, 2]::numeric[])::TEXT", "{1,2}"),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![Value::String((*expected).to_owned())]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
