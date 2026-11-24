//! Data-modifying WITH queries run once and atomically (#823), after the
//! PostgreSQL 18.6 oracle.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn audit_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE mc_events (id INT PRIMARY KEY, payload TEXT)",
            "CREATE TABLE mc_audit (id INT PRIMARY KEY, note TEXT)",
            "INSERT INTO mc_events VALUES (1, 'one')",
        ],
    )
}

#[test]
fn should_run_an_unreferenced_data_modifying_cte_exactly_once() {
    // Arrange
    let fixture = audit_fixture("issue-823-unreferenced-modifying-cte");

    // Act
    let counted = fixture.execute(
        "WITH logged AS (INSERT INTO mc_audit VALUES (10, 'unreferenced') RETURNING id) \
         SELECT count(*) FROM mc_events",
    );
    let audit = fixture.rows("SELECT id, note FROM mc_audit ORDER BY id");

    // Assert
    assert_eq!(
        counted
            .expect("outer query beside an unreferenced CTE")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        audit,
        vec![vec![
            Value::Int64(10),
            Value::String("unreferenced".to_owned())
        ]]
    );
}

#[test]
fn should_discard_every_data_modifying_cte_when_a_later_one_fails() {
    // Arrange
    let fixture = audit_fixture("issue-823-modifying-cte-atomicity");

    // Act
    let failed = fixture.execute(
        "WITH first AS (INSERT INTO mc_audit VALUES (11, 'first') RETURNING id), \
              second AS (INSERT INTO mc_events VALUES (1, 'duplicate') RETURNING id) \
         SELECT 1",
    );
    let audit = fixture.rows("SELECT id FROM mc_audit ORDER BY id");

    // Assert
    assert!(
        matches!(failed, Err(CassieError::UniqueViolation { .. })),
        "the second CTE must report its 23505 duplicate key: {failed:?}"
    );
    assert_eq!(audit, Vec::<Vec<Value>>::new());
}
