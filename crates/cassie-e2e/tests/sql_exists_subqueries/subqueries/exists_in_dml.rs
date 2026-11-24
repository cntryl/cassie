use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn dml_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (a INT, b TEXT)",
            "CREATE TABLE empty_t (z INT)",
            "CREATE TABLE one_t (z INT)",
            "INSERT INTO t (a, b) VALUES (1, 'p')",
            "INSERT INTO t (a, b) VALUES (2, 'q')",
            "INSERT INTO t (a, b) VALUES (3, 'r')",
            "INSERT INTO one_t (z) VALUES (1)",
        ],
    )
}

fn remaining(fixture: &SqlFixture) -> Vec<Vec<Value>> {
    fixture.rows("SELECT a, b FROM t ORDER BY a")
}

fn row(a: i64, b: &str) -> Vec<Value> {
    vec![Value::Int64(a), Value::String(b.to_string())]
}

#[test]
fn should_delete_rows_filtered_by_exists_predicates() {
    // Arrange
    let fixture = dml_fixture("exists_delete");
    let statements = [
        "DELETE FROM t WHERE EXISTS (SELECT 1 FROM empty_t)",
        "DELETE FROM t WHERE a = 1 AND EXISTS (SELECT 1 FROM one_t)",
        "DELETE FROM t WHERE a = 2 AND NOT EXISTS (SELECT 1 FROM empty_t)",
    ];

    // Act
    let succeeded: Vec<bool> = statements
        .iter()
        .map(|sql| fixture.execute(sql).is_ok())
        .collect();

    // Assert
    assert_eq!(succeeded, vec![true; statements.len()]);
    assert_eq!(remaining(&fixture), vec![row(3, "r")]);
}

#[test]
fn should_update_rows_filtered_by_exists_predicates() {
    // Arrange
    let fixture = dml_fixture("exists_update");
    let statements = [
        "UPDATE t SET b = 'x' WHERE EXISTS (SELECT 1 FROM empty_t)",
        "UPDATE t SET b = 'y' WHERE a = 1 AND EXISTS (SELECT 1 FROM one_t)",
        "UPDATE t SET b = 'z' WHERE a = 2 AND NOT EXISTS (SELECT 1 FROM empty_t)",
    ];

    // Act
    let succeeded: Vec<bool> = statements
        .iter()
        .map(|sql| fixture.execute(sql).is_ok())
        .collect();

    // Assert
    assert_eq!(succeeded, vec![true; statements.len()]);
    assert_eq!(
        remaining(&fixture),
        vec![row(1, "y"), row(2, "z"), row(3, "r")]
    );
}

#[test]
fn should_apply_on_conflict_updates_guarded_by_exists() {
    // Arrange
    let fixture = dml_fixture("exists_on_conflict");
    let setup = fixture.execute("CREATE TABLE k (id INT PRIMARY KEY, v TEXT)");
    assert!(setup.is_ok(), "create k");
    assert!(
        fixture
            .execute("INSERT INTO k (id, v) VALUES (1, 'a')")
            .is_ok(),
        "seed k"
    );

    // Act
    let guarded = fixture.execute(
        "INSERT INTO k (id, v) VALUES (1, 'b') ON CONFLICT (id) DO UPDATE SET v = 'b' \
             WHERE EXISTS (SELECT 1 FROM one_t)",
    );

    // Assert
    assert!(guarded.is_ok(), "ON CONFLICT ... WHERE EXISTS should run");
    assert_eq!(
        fixture.rows("SELECT v FROM k"),
        vec![vec![Value::String("b".to_string())]]
    );
}
