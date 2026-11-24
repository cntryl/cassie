//! Pinned PostgreSQL18.6 early conflict-target semantics for the selected RR profile.

use cassie::app::{CassieError, TransactionErrorKind};
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_reject_changed_repeatable_conflict_targets_before_returning() {
    // Arrange
    let mutations: [(bool, &[&str]); 4] = [
        (false, &["INSERT INTO rr_conflicts VALUES (1, 'key', 1)"]),
        (true, &["UPDATE rr_conflicts SET v = 1 WHERE id = 1"]),
        (true, &["UPDATE rr_conflicts SET v = 2 WHERE id = 1"]),
        (
            true,
            &[
                "DELETE FROM rr_conflicts WHERE id = 1",
                "INSERT INTO rr_conflicts VALUES (1, 'key', 1)",
            ],
        ),
    ];
    for target in ["id", "k"] {
        for (seed, mutations) in mutations {
            for action in [
                "DO NOTHING",
                "DO UPDATE SET v = excluded.v WHERE FALSE",
                "DO UPDATE SET v = excluded.v",
            ] {
                let fixture = sql_fixture(
                    "rr-early-conflict",
                    &["CREATE TABLE rr_conflicts (id INT PRIMARY KEY, k TEXT UNIQUE, v INT)"],
                );
                if seed {
                    fixture
                        .execute("INSERT INTO rr_conflicts VALUES (1, 'key', 1)")
                        .expect("old target");
                }
                fixture
                    .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
                    .expect("begin");
                fixture
                    .execute("SELECT 1")
                    .expect("retained view before relation lookup");
                let writer = fixture.cassie.create_session("tester", None);
                for sql in mutations {
                    fixture
                        .cassie
                        .execute_sql(&writer, sql, Vec::new())
                        .expect("competing commit");
                }
                let id = if target == "id" { 1 } else { 2 };

                // Act
                let result = fixture.execute(&format!("INSERT INTO rr_conflicts VALUES ({id}, 'key', 9) ON CONFLICT ({target}) {action} RETURNING id"));
                fixture.execute("ROLLBACK").expect("retire failed owner");

                // Assert
                assert!(
                    matches!(
                        result,
                        Err(CassieError::Transaction {
                            kind: TransactionErrorKind::SerializationFailure,
                            ..
                        })
                    ),
                    "target={target}, action={action}, mutations={mutations:?}"
                );
                assert_eq!(
                    fixture.rows("SELECT count(*) FROM rr_conflicts"),
                    vec![vec![Value::Int64(1)]]
                );
            }
        }
    }
}

#[test]
fn should_allow_a_new_repeatable_row_after_completed_insert_delete() {
    // Arrange
    for target in ["id", "k"] {
        for action in [
            String::new(),
            format!(" ON CONFLICT ({target}) DO NOTHING"),
            format!(" ON CONFLICT ({target}) DO UPDATE SET v = excluded.v WHERE FALSE"),
        ] {
            let fixture = sql_fixture(
                "rr-empty-conflict-control",
                &["CREATE TABLE rr_empty_conflicts (id INT PRIMARY KEY, k TEXT UNIQUE, v INT)"],
            );
            fixture
                .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
                .expect("begin");
            fixture.execute("SELECT 1").expect("old empty view");
            let writer = fixture.cassie.create_session("tester", None);
            for sql in [
                "INSERT INTO rr_empty_conflicts VALUES (1, 'key', 1)",
                "DELETE FROM rr_empty_conflicts WHERE id = 1",
            ] {
                fixture
                    .cassie
                    .execute_sql(&writer, sql, Vec::new())
                    .expect("completed competing mutation");
            }

            // Act
            let inserted = fixture
                .execute(&format!(
                    "INSERT INTO rr_empty_conflicts VALUES (1, 'key', 9){action} RETURNING id"
                ))
                .expect("current target is absent");
            fixture.execute("COMMIT").expect("new row publication");

            // Assert
            assert_eq!(inserted.rows(), &[vec![Value::Int64(1)]]);
            assert_eq!(
                fixture.rows("SELECT id, v FROM rr_empty_conflicts"),
                vec![vec![Value::Int64(1), Value::Int64(9)]]
            );
        }
    }
}

#[test]
fn should_exempt_the_transactions_own_staged_conflict_target() {
    // Arrange
    let fixture = sql_fixture(
        "rr-own-conflict-control",
        &["CREATE TABLE rr_own_conflicts (id INT PRIMARY KEY, v INT)"],
    );
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("INSERT INTO rr_own_conflicts VALUES (1, 1)")
        .expect("own target");
    fixture
        .execute("SAVEPOINT own_target")
        .expect("savepoint retains original view");

    // Act
    let skipped = fixture
        .execute(
            "INSERT INTO rr_own_conflicts VALUES (1, 2) ON CONFLICT (id) DO NOTHING RETURNING id",
        )
        .expect("own skip");
    let changed = fixture.execute("INSERT INTO rr_own_conflicts VALUES (1, 3) ON CONFLICT (id) DO UPDATE SET v = excluded.v RETURNING v").expect("own update");
    fixture
        .execute("ROLLBACK TO SAVEPOINT own_target")
        .expect("restore own target");
    fixture.execute("COMMIT").expect("publish restored own row");

    // Assert
    let empty: &[Vec<Value>] = &[];
    assert_eq!(skipped.rows(), empty);
    assert_eq!(changed.rows(), &[vec![Value::Int64(3)]]);
    assert_eq!(
        fixture.rows("SELECT id, v FROM rr_own_conflicts"),
        vec![vec![Value::Int64(1), Value::Int64(1)]]
    );
}
