//! PostgreSQL 18.6 transaction isolation boundaries tracked by #819.

use super::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;

#[test]
fn should_admit_selected_read_write_repeatable_read_forms() {
    // Arrange
    for begin in [
        "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
    ] {
        let fixture = sql_fixture(
            "issue-819-repeatable-read-isolation",
            &["CREATE TABLE admitted_rr (id INT PRIMARY KEY)"],
        );
        fixture.execute(begin).expect("selected BEGIN form");
        if begin.ends_with("READ ONLY") {
            fixture
                .execute("SET transaction_read_only = off")
                .expect("change mode before the first snapshot");
        }

        // Act
        fixture
            .execute("INSERT INTO admitted_rr VALUES (1)")
            .expect("stage under RR");
        fixture
            .execute("COMMIT")
            .expect("publish through selected fence");
        let rows = fixture
            .execute("SELECT id FROM admitted_rr")
            .expect("committed read");

        // Assert
        assert_eq!(rows.rows(), &[vec![cassie::types::Value::Int64(1)]]);
        assert_eq!(fixture.session.transaction_status(), "idle");
    }
}

#[test]
fn should_reject_read_write_mode_after_a_repeatable_snapshot() {
    // Arrange
    let fixture = sql_fixture("issue-819-mode-after-snapshot", &[]);
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .expect("begin");
    fixture.execute("SELECT 1").expect("first snapshot");

    // Act
    let failure = fixture.execute("SET transaction_read_only = off");
    fixture.execute("ROLLBACK").expect("retire failed block");

    // Assert
    assert!(matches!(
        failure.expect_err("mode fixed by snapshot"),
        cassie::app::CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::ActiveTransaction,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "idle");
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
        .expect_err("SET TRANSACTION remains outside the selected transaction profile");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(_)));
    assert_eq!(session.transaction_status(), "failed");
    fixture
        .cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback the transaction after the rejected setting");
    assert_eq!(session.transaction_status(), "idle");
}

#[test]
fn should_preserve_own_rows_when_repeatable_writes_are_tightened_to_read_only() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-tighten-read-only",
        &["CREATE TABLE tightened_rr (id INT PRIMARY KEY)"],
    );
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("INSERT INTO tightened_rr VALUES (1)")
        .expect("own row");

    // Act
    fixture
        .execute("SET transaction_read_only = on")
        .expect("legal tightening");
    let own = fixture
        .execute("SELECT id FROM tightened_rr")
        .expect("own overlay remains visible");
    fixture
        .execute("COMMIT")
        .expect("publish retained RW owner");
    let published = fixture
        .execute("SELECT id FROM tightened_rr")
        .expect("published row");

    // Assert
    assert_eq!(own.rows(), &[vec![cassie::types::Value::Int64(1)]]);
    assert_eq!(published.rows(), own.rows());
}

#[test]
fn should_keep_the_row_fence_after_repeatable_writes_are_tightened_to_read_only() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-tightened-fence",
        &[
            "CREATE TABLE tightened_conflict (id INT PRIMARY KEY, v INT)",
            "INSERT INTO tightened_conflict VALUES (1, 1)",
        ],
    );
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("UPDATE tightened_conflict SET v = 2 WHERE id = 1")
        .expect("staged row");
    fixture
        .execute("SET transaction_read_only = on")
        .expect("legal tightening");
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE tightened_conflict SET v = v WHERE id = 1",
            Vec::new(),
        )
        .expect("same-payload competing publication");

    // Act
    let failure = fixture.execute("COMMIT");
    let visible = fixture
        .execute("SELECT v FROM tightened_conflict WHERE id = 1")
        .expect("winner");

    // Assert
    assert!(matches!(
        failure.expect_err("mode tightening cannot remove FCW"),
        cassie::app::CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::SerializationFailure,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "idle");
    assert_eq!(visible.rows(), &[vec![cassie::types::Value::Int64(1)]]);
}

#[test]
fn should_retain_write_authority_for_a_restorable_savepoint_mode() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-saved-write-authority",
        &[
            "CREATE TABLE saved_rr (id INT PRIMARY KEY, v INT)",
            "INSERT INTO saved_rr VALUES (1, 1)",
        ],
    );
    for sql in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "SAVEPOINT writable",
        "SET transaction_read_only = on",
        "SELECT 1",
    ] {
        fixture
            .execute(sql)
            .expect("capture while a writable mode can be restored");
    }
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE saved_rr SET v = v WHERE id = 1",
            Vec::new(),
        )
        .expect("same-payload competing publication");

    // Act
    fixture
        .execute("ROLLBACK TO SAVEPOINT writable")
        .expect("restore writable mode");
    fixture
        .execute("UPDATE saved_rr SET v = 2 WHERE id = 1")
        .expect("restored write permission");
    let own = fixture
        .execute("SELECT v FROM saved_rr WHERE id = 1")
        .expect("own overlay");
    let failure = fixture.execute("COMMIT");

    // Assert
    assert_eq!(own.rows(), &[vec![cassie::types::Value::Int64(2)]]);
    assert!(matches!(
        failure.expect_err("capture retained its original FCW authority"),
        cassie::app::CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::SerializationFailure,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "idle");
}
