//! An untyped string literal takes the type of the column it is assigned to,
//! and character columns drop excess trailing spaces (PostgreSQL 18.6).
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE literal_cells (id INT PRIMARY KEY, small SMALLINT, big BIGINT, \
             flag BOOLEAN, ratio FLOAT8, numbers INT[], labels TEXT[])",
        ],
    )
}

const READ: &str = "SELECT id, small, big, flag, ratio, numbers::text, labels::text \
                    FROM literal_cells ORDER BY id";

#[test]
fn should_convert_quoted_literals_to_the_column_type_when_inserting_values() {
    // Arrange
    let fixture = fixture("insert-unknown-literals");

    // Act
    let inserted = fixture.execute(
        "INSERT INTO literal_cells VALUES ('2', '9', '10', 'true', '3.5', '{5,6}', '{x,y}')",
    );
    let rows = fixture.rows(READ);

    // Assert
    assert!(inserted.is_ok(), "quoted literals are assignable");
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(2),
            Value::Int64(9),
            Value::Int64(10),
            Value::Bool(true),
            Value::Float64(3.5),
            Value::String("{5,6}".into()),
            Value::String("{x,y}".into()),
        ]]
    );
}

#[test]
fn should_convert_quoted_literals_to_the_column_type_when_updating() {
    // Arrange
    let fixture = fixture("update-unknown-literals");
    fixture
        .execute("INSERT INTO literal_cells VALUES (1, 1, 1, true, 1.0, ARRAY[1], ARRAY['a'])")
        .expect("seed row");

    // Act
    let updated = fixture.execute(
        "UPDATE literal_cells SET small = '7', flag = 'f', ratio = '2.5', numbers = '{3,4}' \
         WHERE id = 1",
    );
    let rows = fixture.rows(READ);

    // Assert
    assert!(updated.is_ok(), "quoted literals are assignable");
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(1),
            Value::Int64(7),
            Value::Int64(1),
            Value::Bool(false),
            Value::Float64(2.5),
            Value::String("{3,4}".into()),
            Value::String("{a}".into()),
        ]]
    );
}

#[test]
fn should_reject_a_quoted_literal_that_is_not_valid_for_the_column_type() {
    // Arrange
    let fixture = fixture("insert-unknown-literal-invalid");

    // Act
    let malformed = fixture.error("INSERT INTO literal_cells (id) VALUES ('zz')");
    let out_of_range = fixture.error("INSERT INTO literal_cells (id, small) VALUES (4, '99999')");
    let rows = fixture.rows(READ);

    // Assert
    assert_eq!(malformed.sql_state(), "22P02");
    assert_eq!(out_of_range.sql_state(), "22003");
    assert!(rows.is_empty(), "rejected rows are not stored");
}

#[test]
fn should_drop_excess_trailing_spaces_when_assigning_to_a_limited_character_column() {
    // Arrange
    let fixture = sql_fixture(
        "insert-character-trailing-spaces",
        &["CREATE TABLE limited_text (id INT PRIMARY KEY, code VARCHAR(3))"],
    );

    // Act
    let inserted = fixture.execute("INSERT INTO limited_text VALUES (1, 'ab  '), (2, 'abc   ')");
    let updated = fixture.execute("UPDATE limited_text SET code = 'xy    ' WHERE id = 2");
    let rows = fixture.rows("SELECT id, code, length(code) FROM limited_text ORDER BY id");

    // Assert
    assert!(inserted.is_ok(), "only spaces exceed the limit");
    assert!(updated.is_ok(), "only spaces exceed the limit");
    assert_eq!(
        rows,
        vec![
            vec![
                Value::Int64(1),
                Value::String("ab ".into()),
                Value::Int64(3)
            ],
            vec![
                Value::Int64(2),
                Value::String("xy ".into()),
                Value::Int64(3)
            ],
        ]
    );
}

#[test]
fn should_keep_rejecting_non_space_overflow_of_a_limited_character_column() {
    // Arrange
    let fixture = sql_fixture(
        "insert-character-overflow",
        &["CREATE TABLE limited_text (id INT PRIMARY KEY, code VARCHAR(3))"],
    );

    // Act
    let error = fixture.error("INSERT INTO limited_text VALUES (1, 'abcd')");

    // Assert
    assert_eq!(error.sql_state(), "22001");
}
