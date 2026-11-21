//! Array subscript bounds must stay inside PostgreSQL's INT4 limits (SQLSTATE 54000).
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &["CREATE TABLE bounded (id INT PRIMARY KEY, a INT[])"],
    )
}

#[test]
fn should_reject_array_literal_whose_upper_bound_is_int_max() {
    // Arrange
    let fixture = fixture("array-bounds-insert-int-max");

    // Act
    let error = fixture.error("INSERT INTO bounded VALUES (1, '[2147483647:2147483647]={1}')");

    // Assert
    assert_eq!(error.sql_state(), "54000");
    assert!(error
        .to_string()
        .contains("array upper bound is too large: 2147483647"));
}

#[test]
fn should_reject_array_cast_whose_span_exceeds_int4() {
    // Arrange
    let fixture = fixture("array-bounds-cast-span");

    // Act
    let error = fixture.error("SELECT '[-2147483648:2147483646]={1}'::int[]");

    // Assert
    assert_eq!(error.sql_state(), "54000");
}

#[test]
fn should_reject_array_literal_with_upper_bound_below_lower_bound() {
    // Arrange
    let fixture = fixture("array-bounds-inverted");

    // Act
    let error = fixture.error("SELECT '[1:0]={}'::int[]");

    // Assert
    assert_eq!(error.sql_state(), "2202E");
    assert!(error
        .to_string()
        .contains("upper bound cannot be less than lower bound"));
}

#[test]
fn should_reject_element_assignment_past_the_array_size_limit() {
    // Arrange
    let fixture = fixture("array-bounds-element-assign");
    fixture
        .execute("INSERT INTO bounded VALUES (1, '{1,2}')")
        .unwrap();

    // Act
    let error = fixture.error("UPDATE bounded SET a[2147483647] = 1");

    // Assert
    assert_eq!(error.sql_state(), "54000");
}

#[test]
fn should_reject_slice_assignment_past_the_array_size_limit() {
    // Arrange
    let fixture = fixture("array-bounds-slice-assign");
    fixture
        .execute("INSERT INTO bounded VALUES (1, '{1,2}')")
        .unwrap();

    // Act
    let error = fixture.error("UPDATE bounded SET a[2147483646:2147483647] = ARRAY[1,2]");

    // Assert
    assert_eq!(error.sql_state(), "54000");
}

#[test]
fn should_keep_custom_lower_bound_arrays_updatable_by_slice() {
    // Arrange
    let fixture = fixture("array-bounds-custom-lower-guard");
    fixture
        .execute("INSERT INTO bounded VALUES (1, '[5:6]={1,2}')")
        .unwrap();

    // Act
    fixture
        .execute("UPDATE bounded SET a[5:6] = ARRAY[7,8]")
        .unwrap();
    let rows = fixture.rows("SELECT a::text FROM bounded");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("[5:6]={7,8}".into())]]);
}
