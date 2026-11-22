//! `PREPARE` is session-local: it reveals no more than the direct path does, and
//! the read is authorized when the prepared statement executes.

use crate::sequence_state_privileges::{denied, fixture};
use cassie::types::Value;

#[test]
fn should_deny_executing_a_prepared_read_of_a_forbidden_table() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);
    cassie
        .execute_sql(&alice, "PREPARE p AS SELECT ssn FROM secret", vec![])
        .expect("prepare is session-local");

    // Act
    let refused = denied(&cassie, &alice, "EXECUTE p");

    // Assert
    assert!(refused);
}

#[test]
fn should_report_a_missing_table_when_preparing_as_the_direct_path_does() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let direct = cassie.execute_sql(&alice, "SELECT x FROM nosuch", vec![]);
    let prepared = cassie.execute_sql(&alice, "PREPARE q AS SELECT x FROM nosuch", vec![]);

    // Assert
    assert_eq!(
        direct.err().map(|error| error.sql_state()),
        prepared.err().map(|error| error.sql_state())
    );
}

#[test]
fn should_allow_preparing_a_read_of_a_granted_table() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let prepared = cassie.execute_sql(&alice, "PREPARE p AS SELECT v FROM pt", vec![]);
    let executed = cassie.execute_sql(&alice, "EXECUTE p", vec![]);

    // Assert
    assert!(prepared.is_ok());
    assert!(executed.is_ok_and(|result| result.rows() == vec![vec![Value::Int64(1)]]));
}
