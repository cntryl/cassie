//! PostgreSQL 18.6 transaction isolation boundaries tracked by #819.

use super::support_sql_fixture::sql_fixture;
use cassie::app::{CassieError, TransactionErrorKind};

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
fn should_set_repeatable_read_before_the_first_snapshot() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-set-transaction-repeatable-read",
        &[
            "CREATE TABLE set_rr_rows (id INT PRIMARY KEY)",
            "INSERT INTO set_rr_rows VALUES (1)",
        ],
    );
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .execute("BEGIN")
        .expect("begin READ COMMITTED block");
    fixture
        .execute("SHOW transaction_isolation")
        .expect("SHOW does not acquire a query snapshot");
    fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect("select RR before the first query");
    let initial = fixture
        .execute("SELECT count(*) FROM set_rr_rows")
        .expect("capture the RR snapshot");
    fixture
        .cassie
        .execute_sql(&writer, "INSERT INTO set_rr_rows VALUES (2)", Vec::new())
        .expect("publish a concurrent row");

    // Act
    let retained = fixture
        .execute("SELECT count(*) FROM set_rr_rows")
        .expect("read from the retained snapshot");
    fixture.execute("COMMIT").expect("finish RR block");
    let fresh = fixture
        .execute("SELECT count(*) FROM set_rr_rows")
        .expect("read after commit");

    // Assert
    assert_eq!(initial.rows(), &[vec![cassie::types::Value::Int64(1)]]);
    assert_eq!(retained.rows(), initial.rows());
    assert_eq!(fresh.rows(), &[vec![cassie::types::Value::Int64(2)]]);
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_keep_read_uncommitted_as_the_read_committed_alias_when_set() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-set-transaction-read-uncommitted",
        &[
            "CREATE TABLE set_ru_rows (id INT PRIMARY KEY)",
            "INSERT INTO set_ru_rows VALUES (1)",
        ],
    );
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .execute("BEGIN")
        .expect("begin READ COMMITTED block");
    fixture
        .execute("SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED")
        .expect("select the existing alias");
    fixture
        .execute("SELECT count(*) FROM set_ru_rows")
        .expect("first statement view");
    fixture
        .cassie
        .execute_sql(&writer, "INSERT INTO set_ru_rows VALUES (2)", Vec::new())
        .expect("publish a concurrent row");

    // Act
    let refreshed = fixture
        .execute("SELECT count(*) FROM set_ru_rows")
        .expect("READ UNCOMMITTED remains READ COMMITTED");
    fixture.execute("COMMIT").expect("finish transaction");

    // Assert
    assert_eq!(refreshed.rows(), &[vec![cassie::types::Value::Int64(2)]]);
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_set_transaction_isolation_after_a_query() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-isolation-after-query", &[]);
    fixture.execute("BEGIN").expect("begin");
    fixture.execute("SELECT 1").expect("run first query");

    // Act
    let failure = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("isolation must be selected before a query");

    // Assert
    assert!(matches!(
        failure,
        CassieError::Transaction {
            kind: TransactionErrorKind::ActiveTransaction,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "failed");
    fixture
        .execute("ROLLBACK")
        .expect("recover the failed block");
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_set_transaction_isolation_after_a_savepoint() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-isolation-after-savepoint", &[]);
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("SAVEPOINT before_isolation")
        .expect("open a subtransaction");

    // Act
    let failure = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("isolation cannot change inside a subtransaction");

    // Assert
    assert!(matches!(
        failure,
        CassieError::Transaction {
            kind: TransactionErrorKind::ActiveTransaction,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "failed");
    fixture
        .execute("ROLLBACK")
        .expect("recover the failed block");
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_set_transaction_isolation_outside_an_explicit_block() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-isolation-outside-block", &[]);

    // Act
    let failure = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .expect_err("SET TRANSACTION requires an explicit block");

    // Assert
    assert!(matches!(
        failure,
        CassieError::Transaction {
            kind: TransactionErrorKind::NoActiveTransaction,
            ..
        }
    ));
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_reject_serializable_set_transaction_isolation_deterministically() {
    // Arrange
    let fixture = sql_fixture("issue-819-set-serializable", &[]);
    fixture.execute("BEGIN").expect("begin");

    // Act
    let failure = fixture
        .execute("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .expect_err("SERIALIZABLE is outside the selected profile");

    // Assert
    assert!(matches!(failure, CassieError::Unsupported(_)));
    assert_eq!(fixture.session.transaction_status(), "failed");
    fixture
        .execute("ROLLBACK")
        .expect("recover the failed block");
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
