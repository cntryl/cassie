//! A RIGHT or FULL JOIN LATERAL may not read the left side, as in PostgreSQL.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE rl (id INT, n INT)",
            "INSERT INTO rl VALUES (1, 2)",
        ],
    )
}

#[test]
fn should_reject_right_join_lateral_function_that_reads_the_left_side() {
    // Arrange
    let fixture = fixture("lateral-right-function");

    // Act
    let error = fixture.error(
        "SELECT rl.id, g.v FROM rl RIGHT JOIN LATERAL generate_series(1, rl.n) AS g(v) ON true",
    );

    // Assert
    assert_eq!(error.sql_state(), "42P01");
}

#[test]
fn should_reject_full_join_lateral_function_that_reads_the_left_side() {
    // Arrange
    let fixture = fixture("lateral-full-function");

    // Act
    let error = fixture.error(
        "SELECT rl.id, g.v FROM rl FULL JOIN LATERAL generate_series(1, rl.n) AS g(v) ON true",
    );

    // Assert
    assert_eq!(error.sql_state(), "42P01");
}

#[test]
fn should_reject_right_join_lateral_subquery_that_reads_the_left_side() {
    // Arrange
    let fixture = fixture("lateral-right-subquery");

    // Act
    let error = fixture
        .error("SELECT rl.id, d.x FROM rl RIGHT JOIN LATERAL (SELECT rl.n AS x) AS d ON true");

    // Assert
    assert_eq!(error.sql_state(), "42P01");
}

#[test]
fn should_accept_right_join_lateral_function_that_ignores_the_left_side() {
    // Arrange
    let fixture = fixture("lateral-right-independent");

    // Act
    let rows = fixture
        .rows("SELECT rl.id, g.v FROM rl RIGHT JOIN LATERAL generate_series(1, 2) AS g(v) ON true");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_accept_left_join_lateral_function_that_reads_the_left_side() {
    // Arrange
    let fixture = fixture("lateral-left-reads");

    // Act
    let rows = fixture.rows(
        "SELECT rl.id, g.v FROM rl LEFT JOIN LATERAL generate_series(1, rl.n) AS g(v) ON true",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2)],
        ]
    );
}
