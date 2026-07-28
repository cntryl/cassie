#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint,
};
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_transition_session_state_for_transaction_control() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let begin = cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let during = session.transaction_status();
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after = session.transaction_status();

        // Assert
        assert_eq!(begin.command, "BEGIN");
        assert_eq!(during, "in_transaction");
        assert_eq!(commit.command, "COMMIT");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_restore_idle_state_on_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        let rollback = cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let after = session.transaction_status();

        // Assert
        assert_eq!(rollback.command, "ROLLBACK");
        assert_eq!(after, "idle");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_autocommit_writes_visible_after_success() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_autocommit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_autocommit (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_autocommit (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM transaction_autocommit", vec![])
            .unwrap();

        // Assert
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsupported_transaction_control_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let statement = cassie.execute_sql(
            &session,
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            vec![],
        );

        // Assert
        assert!(statement.is_err());
        assert!(statement.unwrap_err().to_string().contains("unsupported"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_discard_later_writes() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_savepoint_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_rollback (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_rollback ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_release_savepoint_prevent_later_rollback_to_it() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_savepoint_release");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();

        // Act
        cassie
            .execute_sql(&session, "RELEASE SAVEPOINT sp", vec![])
            .unwrap();
        let rollback = cassie.execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![]);

        // Assert
        assert!(rollback.is_err());
        assert!(rollback.unwrap_err().to_string().contains("savepoint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_savepoint_outside_transaction() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_savepoint_outside");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let savepoint = cassie.execute_sql(&session, "SAVEPOINT sp", vec![]);

        // Assert
        assert!(savepoint.is_err());
        assert!(savepoint
            .unwrap_err()
            .to_string()
            .contains("active transaction"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rollback_to_savepoint_recover_failed_transaction() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_savepoint_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_savepoint_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(&session, "SAVEPOINT sp", vec![])
            .unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT sp", vec![])
            .unwrap();
        let status = session.transaction_status();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_savepoint_failed_recovery (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_savepoint_failed_recovery",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(status, "in_transaction");
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_advisory_lock_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_advisory_lock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_advisory_lock (id INT)",
                vec![],
            )
            .unwrap();

        // Act
        let lock = cassie.execute_sql(
            &session,
            "SELECT pg_advisory_lock(1) FROM transaction_advisory_lock",
            vec![],
        );

        // Assert
        assert!(lock.is_err());
        assert!(lock
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_writes_after_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_rollback_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_rollback_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_rollback_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_rollback_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_work_after_transaction_error_until_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_failed_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_state (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_failed_state (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());

        // Act
        let selected = cassie.execute_sql(
            &session,
            "SELECT title FROM transaction_failed_state",
            vec![],
        );

        // Assert
        assert!(selected.is_err());
        assert!(selected
            .unwrap_err()
            .to_string()
            .contains("rollback required"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_commit_multi_collection_transaction_atomically() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_multi_collection_atomic");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE tx_multi_a (id TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE tx_multi_b (email TEXT)", vec![])
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO tx_multi_a (id) VALUES ('row-1')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO tx_multi_b (email) VALUES ('alice@example.com')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();

        // Assert
        let collected_from_a = cassie
            .execute_sql(&session, "SELECT id FROM tx_multi_a", vec![])
            .unwrap()
            .rows;
        assert_eq!(collected_from_a.len(), 1);
        let collected_from_b = cassie
            .execute_sql(
                &session,
                "SELECT email FROM tx_multi_b ORDER BY email",
                vec![],
            )
            .unwrap()
            .rows;
        assert_eq!(
            collected_from_b,
            vec![vec![Value::String("alice@example.com".into())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_bump_data_epoch_for_no_op_delete() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_no_op_delete_data_epoch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE no_op_delete_epoch (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO no_op_delete_epoch (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        let before_epoch = cassie.midge.data_epoch().unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM no_op_delete_epoch WHERE id = 2",
                vec![],
            )
            .unwrap();
        let after_epoch = cassie.midge.data_epoch().unwrap();

        // Assert
        assert_eq!(deleted.command, "DELETE 0");
        assert_eq!(before_epoch, after_epoch);
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM no_op_delete_epoch ORDER BY id",
                vec![],
            )
            .unwrap();
        assert_eq!(rows.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_replace_same_id_in_single_statement() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_same_id_replace");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE same_id_replace (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO same_id_replace (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let replaced = cassie
            .execute_sql(
                &session,
                "INSERT INTO same_id_replace (id, title) VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title",
                vec![],
            )
            .unwrap();

        // Assert
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM same_id_replace WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(replaced.command, "INSERT 0 1");
        assert_eq!(rows.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_allow_work_after_failed_transaction_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_failed_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_failed_recovery (title TEXT NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        let failed_insert = cassie.execute_sql(
            &session,
            "INSERT INTO transaction_failed_recovery (title) VALUES (NULL)",
            vec![],
        );
        assert!(failed_insert.is_err());
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_failed_recovery",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_update_after_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_update_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_rollback SET title = 'beta'",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_rollback",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_discard_transaction_delete_after_rollback() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_delete_rollback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_rollback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_rollback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_rollback WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_rollback",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_update_before_commit() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_update_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_update_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_read_your_writes SET title = 'beta'",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_update_read_your_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_delete_before_commit() {
    // Arrange
    use_local_storage();
    let path = data_dir("transaction_delete_read_your_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_delete_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_delete_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_delete_read_your_writes WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_delete_read_your_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}
