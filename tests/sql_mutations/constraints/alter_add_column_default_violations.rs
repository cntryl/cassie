use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn two_rows(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "INSERT INTO t VALUES (1), (2)",
        ],
    )
}

fn assert_table_unchanged(fixture: &SqlFixture, state: &str, sqlstate: &str) {
    assert_eq!(state, sqlstate);
    let column_error = fixture.execute("SELECT c FROM t");
    assert!(
        column_error.is_err(),
        "column c must be absent after the rejected ALTER"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM t ORDER BY id"),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_reject_add_column_whose_default_violates_its_check() {
    // Arrange
    let fixture = two_rows("add-col-default-check");

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT DEFAULT -1 CHECK (c > 0)")
        .sql_state();

    // Assert
    assert_table_unchanged(&fixture, state, "23514");
}

#[test]
fn should_allow_add_column_after_rejected_check_default() {
    // Arrange
    let fixture = two_rows("add-col-default-check-retry");
    let _rejected = fixture.error("ALTER TABLE t ADD COLUMN c INT DEFAULT -1 CHECK (c > 0)");

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT DEFAULT 3 CHECK (c > 0)")
        .expect("retry add column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT c FROM t ORDER BY id"),
        vec![vec![Value::Int64(3)], vec![Value::Int64(3)]]
    );
}

#[test]
fn should_backfill_add_column_default_that_satisfies_its_check() {
    // Arrange
    let fixture = two_rows("add-col-default-check-ok");

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT DEFAULT 5 CHECK (c > 0)")
        .expect("add column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT c FROM t ORDER BY id"),
        vec![vec![Value::Int64(5)], vec![Value::Int64(5)]]
    );
}

#[test]
fn should_reject_unique_add_column_with_constant_default_on_two_rows() {
    // Arrange
    let fixture = two_rows("add-col-default-unique");

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT UNIQUE DEFAULT 7")
        .sql_state();

    // Assert
    assert_table_unchanged(&fixture, state, "23505");
}

#[test]
fn should_allow_add_column_after_rejected_unique_default() {
    // Arrange
    let fixture = two_rows("add-col-default-unique-retry");
    let _rejected = fixture.error("ALTER TABLE t ADD COLUMN c INT UNIQUE DEFAULT 7");

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT UNIQUE")
        .expect("retry add column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT c FROM t ORDER BY id"),
        vec![vec![Value::Null], vec![Value::Null]]
    );
}

#[test]
fn should_allow_unique_add_column_with_constant_default_on_one_row() {
    // Arrange
    let fixture = sql_fixture(
        "add-col-default-unique-one",
        &[
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "INSERT INTO t VALUES (1)",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT UNIQUE DEFAULT 7")
        .expect("add column");

    // Assert
    assert_eq!(fixture.rows("SELECT c FROM t"), vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_allow_unique_add_column_with_constant_default_on_empty_table() {
    // Arrange
    let fixture = sql_fixture(
        "add-col-default-unique-empty",
        &["CREATE TABLE t (id INT PRIMARY KEY)"],
    );

    // Act
    let result = fixture.execute("ALTER TABLE t ADD COLUMN c INT UNIQUE DEFAULT 7");

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_reject_add_column_whose_default_has_no_referenced_row() {
    // Arrange
    let fixture = sql_fixture(
        "add-col-default-fk",
        &[
            "CREATE TABLE p (id INT PRIMARY KEY)",
            "INSERT INTO p VALUES (1), (999)",
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "INSERT INTO t VALUES (1), (2)",
        ],
    );

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT REFERENCES p(id) DEFAULT 5")
        .sql_state();

    // Assert
    assert_table_unchanged(&fixture, state, "23503");
}

#[test]
fn should_backfill_add_column_default_that_has_a_referenced_row() {
    // Arrange
    let fixture = sql_fixture(
        "add-col-default-fk-ok",
        &[
            "CREATE TABLE p (id INT PRIMARY KEY)",
            "INSERT INTO p VALUES (1), (5)",
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "INSERT INTO t VALUES (1), (2)",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT REFERENCES p(id) DEFAULT 5")
        .expect("add column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT c FROM t ORDER BY id"),
        vec![vec![Value::Int64(5)], vec![Value::Int64(5)]]
    );
}

#[test]
fn should_reject_not_null_add_column_without_default_on_populated_table() {
    // Arrange
    let fixture = two_rows("add-col-not-null-no-default");

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT NOT NULL")
        .sql_state();

    // Assert
    assert_table_unchanged(&fixture, state, "23502");
}

#[test]
fn should_reject_not_null_add_column_with_null_default_on_populated_table() {
    // Arrange
    let fixture = two_rows("add-col-not-null-null-default");

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT NOT NULL DEFAULT NULL")
        .sql_state();

    // Assert
    assert_table_unchanged(&fixture, state, "23502");
}

#[test]
fn should_backfill_not_null_add_column_with_default_on_populated_table() {
    // Arrange
    let fixture = two_rows("add-col-not-null-default");

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN c INT NOT NULL DEFAULT 4")
        .expect("add column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT c FROM t ORDER BY id"),
        vec![vec![Value::Int64(4)], vec![Value::Int64(4)]]
    );
}

#[test]
fn should_allow_not_null_add_column_without_default_on_empty_table() {
    // Arrange
    let fixture = sql_fixture(
        "add-col-not-null-empty",
        &["CREATE TABLE t (id INT PRIMARY KEY)"],
    );

    // Act
    let result = fixture.execute("ALTER TABLE t ADD COLUMN c INT NOT NULL");

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_reject_check_violating_add_column_default_inside_a_transaction() {
    // Arrange
    let fixture = two_rows("add-col-default-check-tx");
    fixture.execute("BEGIN").expect("begin");

    // Act
    let state = fixture
        .error("ALTER TABLE t ADD COLUMN c INT DEFAULT -1 CHECK (c > 0)")
        .sql_state();
    fixture.execute("ROLLBACK").expect("rollback");

    // Assert
    assert_eq!(state, "23514");
    assert!(fixture.execute("SELECT c FROM t").is_err());
}
