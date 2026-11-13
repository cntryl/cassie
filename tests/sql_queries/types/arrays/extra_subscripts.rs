//! More subscripts than array dimensions yield NULL, or an empty array when
//! any subscript is a slice (PostgreSQL 18.6).
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_return_null_when_index_subscripts_exceed_the_array_dimensions() {
    // Arrange
    let fixture = sql_fixture(
        "array-extra-index-subscripts",
        &[
            "CREATE TABLE subscript_rows (id INT PRIMARY KEY, numbers INT[])",
            "INSERT INTO subscript_rows VALUES (1, ARRAY[1,2,3])",
        ],
    );

    // Act
    let literal = fixture.execute("SELECT (ARRAY[1,2,3])[1][2] IS NULL");
    let column = fixture.execute("SELECT numbers[1][2] IS NULL FROM subscript_rows");

    // Assert
    assert!(literal.is_ok_and(|result| result.rows() == [vec![Value::Bool(true)]]));
    assert!(column.is_ok_and(|result| result.rows() == [vec![Value::Bool(true)]]));
}

#[test]
fn should_return_an_empty_array_when_slice_subscripts_exceed_the_array_dimensions() {
    // Arrange
    let fixture = sql_fixture("array-extra-slice-subscripts", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT ((ARRAY[1,2,3])[1:2][1:1])::text, ((ARRAY[1,2,3])[1][1:2])::text, \
         (ARRAY[[1,2],[3,4]])[1][2]",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("{}".into()),
            Value::String("{}".into()),
            Value::Int64(2),
        ]]
    );
}
