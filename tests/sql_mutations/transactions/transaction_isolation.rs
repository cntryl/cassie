//! PostgreSQL 18.6 transaction isolation boundaries tracked by #819.

use super::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;

#[test]
fn should_reject_repeatable_read_transaction_isolation() {
    // Arrange
    let fixture = sql_fixture("issue-819-repeatable-read-isolation", &[]);
    let session = fixture.session.clone();

    // Act
    let error = fixture
        .cassie
        .execute_sql(
            &session,
            "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
            vec![],
        )
        .expect_err("repeatable read is outside the selected transaction profile");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(_)));
    assert_eq!(session.transaction_status(), "idle");
}

#[test]
fn should_reject_repeatable_read_set_transaction_before_snapshot() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-transaction-repeatable-read", &[]);
    let session = fixture.session.clone();
    fixture
        .cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin with the default READ COMMITTED mode");

    // Act
    let error = fixture
        .cassie
        .execute_sql(
            &session,
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            vec![],
        )
        .expect_err("SET TRANSACTION must reject the unsupported isolation level");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(_)));
    assert_eq!(session.transaction_status(), "failed");
    fixture
        .cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback the transaction after the rejected setting");
    assert_eq!(session.transaction_status(), "idle");
}
