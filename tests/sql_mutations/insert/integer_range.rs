//! Integer overflow on a write is `22003 numeric_value_out_of_range`, never
//! the vector-validation state (PostgreSQL 18.6).
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE io (id INT, s SMALLINT, i INT, b BIGINT)",
            "INSERT INTO io (id, i) VALUES (1, 1)",
        ],
    )
}

fn assert_out_of_range(error: &CassieError, message: &str) {
    assert_eq!(error.sql_state(), "22003");
    assert!(
        error.to_string().ends_with(message),
        "message does not end with {message}"
    );
}

#[test]
fn should_report_integer_out_of_range_on_insert() {
    // Arrange
    let fixture = fixture("int-range-insert");

    // Act
    let error = fixture.error("INSERT INTO io (id, i) VALUES (2, 3000000000)");

    // Assert
    assert_out_of_range(&error, "integer out of range");
}

#[test]
fn should_report_smallint_out_of_range_on_insert() {
    // Arrange
    let fixture = fixture("smallint-range-insert");

    // Act
    let error = fixture.error("INSERT INTO io (id, s) VALUES (2, 40000)");

    // Assert
    assert_out_of_range(&error, "smallint out of range");
}

#[test]
fn should_report_bigint_out_of_range_on_insert() {
    // Arrange
    let fixture = fixture("bigint-range-insert");

    // Act
    let error = fixture.error("INSERT INTO io (id, b) VALUES (2, 9223372036854775808)");

    // Assert
    assert_out_of_range(&error, "bigint out of range");
}

#[test]
fn should_report_integer_out_of_range_on_update_literal() {
    // Arrange
    let fixture = fixture("int-range-update-literal");

    // Act
    let error = fixture.error("UPDATE io SET i = 3000000000 WHERE id = 1");

    // Assert
    assert_out_of_range(&error, "integer out of range");
}

#[test]
fn should_report_integer_out_of_range_on_update_expression() {
    // Arrange
    let fixture = fixture("int-range-update-expression");

    // Act
    let error = fixture.error("UPDATE io SET i = i + 2147483647 WHERE id = 1");

    // Assert
    assert_out_of_range(&error, "integer out of range");
}

#[test]
fn should_report_integer_out_of_range_for_bound_parameter() {
    // Arrange
    let fixture = fixture("int-range-parameter");

    // Act
    let error = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO io (id, i) VALUES (2, $1)",
            vec![Value::Int64(3_000_000_000)],
        )
        .expect_err("overflowing parameter");

    // Assert
    assert_eq!(error.sql_state(), "22003");
}

#[test]
fn should_report_integer_out_of_range_on_copy() {
    // Arrange
    let fixture = fixture("int-range-copy");
    let statement = CopyStatement {
        table: "io".to_string(),
        columns: vec!["id".to_string(), "i".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let error = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"2,3000000000\n")
        .expect_err("overflowing COPY cell");

    // Assert
    assert_eq!(error.sql_state(), "22003");
}

#[test]
fn should_accept_integer_at_the_range_boundary() {
    // Arrange
    let fixture = fixture("int-range-boundary");

    // Act
    let rows =
        fixture.rows("INSERT INTO io (id, i) VALUES (2, 2147483647), (3, -2147483648) RETURNING i");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(2_147_483_647)],
            vec![Value::Int64(-2_147_483_648)]
        ]
    );
}
