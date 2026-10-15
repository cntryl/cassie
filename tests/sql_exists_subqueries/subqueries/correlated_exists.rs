use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn orders_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE users (uid INT, uname TEXT)",
            "CREATE TABLE orders (oid INT, uid INT)",
            "INSERT INTO users (uid, uname) VALUES (1, 'a')",
            "INSERT INTO users (uid, uname) VALUES (2, 'b')",
            "INSERT INTO users (uid, uname) VALUES (3, 'c')",
            "INSERT INTO orders (oid, uid) VALUES (10, 1)",
            "INSERT INTO orders (oid, uid) VALUES (11, 3)",
        ],
    )
}

fn ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
    rows.into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            other => panic!("expected an integer cell, got {other:?}"),
        })
        .collect()
}

#[test]
fn should_evaluate_correlated_exists_per_outer_row() {
    // Arrange
    let fixture = orders_fixture("correlated_exists");

    // Act
    let exists = ints(fixture.rows(
        "SELECT uid FROM users WHERE EXISTS \
             (SELECT 1 FROM orders WHERE orders.uid = users.uid) ORDER BY uid",
    ));
    let not_exists = ints(fixture.rows(
        "SELECT uid FROM users WHERE NOT EXISTS \
             (SELECT 1 FROM orders WHERE orders.uid = users.uid) ORDER BY uid",
    ));
    let unqualified_outer = ints(fixture.rows(
        "SELECT uid FROM users WHERE EXISTS \
             (SELECT 1 FROM orders WHERE orders.uid = users.uid AND uname = 'c') ORDER BY uid",
    ));
    let conjunct = ints(fixture.rows(
        "SELECT uid FROM users WHERE uid > 1 AND EXISTS \
             (SELECT 1 FROM orders WHERE orders.uid = users.uid) ORDER BY uid",
    ));

    let inner_scope_first = ints(fixture.rows(
        "SELECT uid FROM users WHERE EXISTS \
             (SELECT 1 FROM orders WHERE users.uid = 1 AND uid = 3) ORDER BY uid",
    ));

    // Assert
    assert_eq!(exists, vec![1, 3]);
    assert_eq!(not_exists, vec![2]);
    assert_eq!(unqualified_outer, vec![3]);
    assert_eq!(conjunct, vec![3]);
    assert_eq!(inner_scope_first, vec![1]);
}
