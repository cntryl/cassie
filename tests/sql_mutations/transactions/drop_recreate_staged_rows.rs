//! Rows staged before DROP TABLE must not reach a same-named table created later in the transaction.

use super::super::support_sql_fixture::SqlFixture;
use super::sql_fixture;
use cassie::types::Value;

fn ids(values: &[i64]) -> Vec<Vec<Value>> {
    values.iter().map(|id| vec![Value::Int64(*id)]).collect()
}

fn run(fixture: &SqlFixture, statements: &[&str]) {
    for statement in statements {
        fixture.execute(statement).expect("statement succeeds");
    }
}

const DROP_RECREATE: [&str; 5] = [
    "BEGIN",
    "INSERT INTO dr VALUES (1)",
    "DROP TABLE dr",
    "CREATE TABLE dr (id INT PRIMARY KEY)",
    "INSERT INTO dr VALUES (2)",
];

#[test]
fn should_hide_pre_drop_rows_from_recreated_table_within_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-in-tx",
        &["CREATE TABLE dr (id INT PRIMARY KEY)"],
    );
    run(&fixture, &DROP_RECREATE);

    // Act
    let rows = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, ids(&[2]));
}

#[test]
fn should_not_publish_pre_drop_rows_into_recreated_table_at_commit() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-commit",
        &["CREATE TABLE dr (id INT PRIMARY KEY)"],
    );
    run(&fixture, &DROP_RECREATE);

    // Act
    fixture.execute("COMMIT").expect("commit");
    let rows = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, ids(&[2]));
}

#[test]
fn should_leave_recreated_table_empty_when_only_pre_drop_rows_were_staged() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-empty",
        &["CREATE TABLE dr (id INT PRIMARY KEY)"],
    );
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr VALUES (1)",
            "DROP TABLE dr",
            "CREATE TABLE dr (id INT PRIMARY KEY)",
            "COMMIT",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id FROM dr");

    // Assert
    assert_eq!(rows, Vec::<Vec<Value>>::new());
}

#[test]
fn should_keep_only_final_incarnation_rows_for_table_recreated_inside_one_transaction() {
    // Arrange
    let fixture = sql_fixture("drop-recreate-all-in-tx", &[]);
    run(
        &fixture,
        &[
            "BEGIN",
            "CREATE TABLE dr (id INT PRIMARY KEY)",
            "INSERT INTO dr VALUES (1)",
            "DROP TABLE dr",
            "CREATE TABLE dr (id INT PRIMARY KEY)",
            "INSERT INTO dr VALUES (2)",
            "COMMIT",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, ids(&[2]));
}

#[test]
fn should_accept_key_reused_after_table_recreated_in_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-key-reuse",
        &["CREATE TABLE dr (id INT PRIMARY KEY)"],
    );
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr VALUES (1)",
            "DROP TABLE dr",
            "CREATE TABLE dr (id INT PRIMARY KEY)",
            "INSERT INTO dr VALUES (1)",
            "COMMIT",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, ids(&[1]));
}

#[test]
fn should_restore_original_rows_when_drop_recreate_transaction_rolls_back() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-rollback",
        &[
            "CREATE TABLE dr (id INT PRIMARY KEY)",
            "INSERT INTO dr VALUES (7)",
        ],
    );
    run(&fixture, &DROP_RECREATE);

    // Act
    fixture.execute("ROLLBACK").expect("rollback");
    let rows = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, ids(&[7]));
}

#[test]
fn should_restore_staged_rows_when_rolling_back_to_savepoint_before_drop() {
    // Arrange
    let fixture = sql_fixture("drop-savepoint", &["CREATE TABLE dr (id INT PRIMARY KEY)"]);
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr VALUES (1)",
            "SAVEPOINT s",
            "DROP TABLE dr",
            "ROLLBACK TO s",
        ],
    );

    // Act
    let in_transaction = fixture.rows("SELECT id FROM dr ORDER BY id");
    fixture.execute("COMMIT").expect("commit");
    let committed = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(in_transaction, ids(&[1]));
    assert_eq!(committed, ids(&[1]));
}

#[test]
fn should_hide_pre_drop_rows_from_recreated_column_table() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-columnar",
        &["CREATE TABLE dr (id INT PRIMARY KEY) USING columnar"],
    );
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr VALUES (1)",
            "DROP TABLE dr",
            "CREATE TABLE dr (id INT PRIMARY KEY) USING columnar",
            "INSERT INTO dr VALUES (2)",
        ],
    );

    // Act
    let in_transaction = fixture.rows("SELECT id FROM dr ORDER BY id");
    fixture.execute("COMMIT").expect("commit");
    let committed = fixture.rows("SELECT id FROM dr ORDER BY id");

    // Assert
    assert_eq!(in_transaction, ids(&[2]));
    assert_eq!(committed, ids(&[2]));
}

#[test]
fn should_not_surface_old_payload_columns_in_recreate_with_different_column_list() {
    // Arrange
    let fixture = sql_fixture(
        "drop-recreate-new-columns",
        &["CREATE TABLE dr (id INT PRIMARY KEY, label TEXT)"],
    );
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr VALUES (1, 'old')",
            "DROP TABLE dr",
            "CREATE TABLE dr (id INT PRIMARY KEY, amount INT)",
            "INSERT INTO dr VALUES (2, 20)",
            "COMMIT",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id, amount FROM dr ORDER BY id");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2), Value::Int64(20)]]);
}

#[test]
fn should_not_carry_pre_drop_rows_into_table_renamed_over_dropped_name() {
    // Arrange
    let fixture = sql_fixture(
        "drop-rename-over",
        &[
            "CREATE TABLE dr_a (id INT PRIMARY KEY)",
            "CREATE TABLE dr_b (id INT PRIMARY KEY)",
        ],
    );
    run(
        &fixture,
        &[
            "BEGIN",
            "INSERT INTO dr_b VALUES (1)",
            "DROP TABLE dr_b",
            "INSERT INTO dr_a VALUES (2)",
            "ALTER TABLE dr_a RENAME TO dr_b",
        ],
    );

    // Act
    let in_transaction = fixture.rows("SELECT id FROM dr_b ORDER BY id");
    fixture.execute("COMMIT").expect("commit");
    let committed = fixture.rows("SELECT id FROM dr_b ORDER BY id");

    // Assert
    assert_eq!(in_transaction, ids(&[2]));
    assert_eq!(committed, ids(&[2]));
}
