//! PostgreSQL 18.6 transaction isolation boundaries tracked by #819.

use super::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::types::Value;

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
fn should_reject_set_repeatable_read_outside_an_explicit_transaction() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-transaction-outside-block", &[]);

    // Act
    let error = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("SET TRANSACTION requires an explicit block");

    // Assert
    assert!(matches!(error, CassieError::Unsupported(_)));
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_set_repeatable_read_before_the_first_snapshot_in_an_explicit_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-set-transaction-repeatable-read",
        &[
            "CREATE TABLE set_rr_rows (id INT PRIMARY KEY, value TEXT)",
            "INSERT INTO set_rr_rows VALUES (1, 'before')",
        ],
    );
    let observer = fixture.cassie.create_session("root", None);
    fixture
        .execute("BEGIN")
        .expect("begin with default READ COMMITTED");

    // Act
    fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect("select RR before the first snapshot");
    let isolation = fixture
        .execute("SHOW transaction_isolation")
        .expect("read the selected transaction isolation");
    let before = fixture
        .execute("SELECT value FROM set_rr_rows WHERE id = 1")
        .expect("capture the first RR snapshot");
    fixture
        .cassie
        .execute_sql(
            &observer,
            "UPDATE set_rr_rows SET value = 'after' WHERE id = 1",
            vec![],
        )
        .expect("commit a concurrent row update");
    let retained = fixture
        .execute("SELECT value FROM set_rr_rows WHERE id = 1")
        .expect("read again through the retained RR snapshot");
    fixture
        .execute("COMMIT")
        .expect("commit the read-only transaction");
    let current = fixture
        .execute("SELECT value FROM set_rr_rows WHERE id = 1")
        .expect("read the current row after commit");

    // Assert
    assert_eq!(
        isolation.rows(),
        &[vec![Value::String("repeatable read".into())]]
    );
    assert_eq!(before.rows(), &[vec![Value::String("before".into())]]);
    assert_eq!(retained.rows(), &[vec![Value::String("before".into())]]);
    assert_eq!(current.rows(), &[vec![Value::String("after".into())]]);
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_set_repeatable_read_after_snapshot_capture() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-transaction-after-snapshot", &[]);
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("SELECT 1")
        .expect("capture the first snapshot");

    // Act
    let error = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("transaction isolation cannot change after snapshot capture");
    fixture
        .execute("ROLLBACK")
        .expect("recover the failed transaction");

    // Assert
    assert!(matches!(
        error,
        CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::ActiveTransaction,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_set_repeatable_read_after_savepoint_before_snapshot_capture() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-transaction-after-savepoint", &[]);
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("SAVEPOINT before_mode")
        .expect("create a savepoint without capturing a snapshot");

    // Act
    let error = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("transaction isolation cannot change in a subtransaction");
    fixture
        .execute("ROLLBACK TO SAVEPOINT before_mode")
        .expect("recover to the savepoint");
    let isolation = fixture
        .execute("SHOW transaction_isolation")
        .expect("read the restored isolation");
    fixture.execute("ROLLBACK").expect("finish the block");

    // Assert
    assert!(matches!(
        error,
        CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::ActiveTransaction,
            ..
        }
    ));
    assert_eq!(
        isolation.rows(),
        &[vec![Value::String("read committed".into())]]
    );
    assert_eq!(fixture.session.transaction_status(), "idle");
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
