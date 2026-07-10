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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
fn should_hide_transaction_writes_from_other_sessions_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_uncommitted_visibility");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_uncommitted_visibility (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_uncommitted_visibility (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_uncommitted_visibility",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_writes_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_read_your_writes");
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
                "CREATE TABLE transaction_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_read_your_writes",
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
fn should_persist_transaction_writes_after_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_commit_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_commit_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_commit_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        cassie.execute_sql(&writer, "COMMIT", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_commit_writes",
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
fn should_keep_transaction_insert_out_of_storage_until_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_storage_routing");
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
                "CREATE TABLE transaction_storage_routing (title TEXT)",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("transaction_storage_routing")
            .expect("catalog collection")
            .collection;
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_storage_routing (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .unwrap();
        let row_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };
        let before_commit = cassie.midge.get_document(&collection, &row_id).unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after_commit = cassie.midge.get_document(&collection, &row_id).unwrap();

        // Assert
        assert!(before_commit.is_none());
        assert_eq!(
            after_commit.unwrap().payload["title"],
            serde_json::Value::String("alpha".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_work_after_transaction_error_until_rollback() {
    // Arrange
    with_fallback();
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
fn should_apply_multi_collection_transaction_commit_atomically() {
    // Arrange
    with_fallback();
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
        let before_epoch = cassie.midge.data_epoch().unwrap_or(0);
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
        let after_epoch = cassie.midge.data_epoch().unwrap();

        // Assert
        assert_eq!(after_epoch, before_epoch.saturating_add(1));
        let collected_from_a = cassie
            .execute_sql(&session, "SELECT id FROM tx_multi_a", vec![])
            .unwrap()
            .rows;
        assert_eq!(collected_from_a.len(), 1);
        assert_eq!(collected_from_a[0].len(), 1);
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
            vec![vec![Value::String("alice@example.com".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_bump_data_epoch_for_no_op_delete() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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

#[test]
fn should_not_persist_row_when_row_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_row_failpoint");
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
                "CREATE TABLE write_row_failpoint (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_row_failpoint (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(&session, "SELECT id FROM write_row_failpoint", vec![])
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_row_failpoint (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT title FROM write_row_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_scalar_index_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_scalar_index_failpoint");
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
                "CREATE TABLE write_scalar_index_failpoint (id INT PRIMARY KEY, email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_scalar_index_failpoint_email_idx ON write_scalar_index_failpoint USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::ScalarIndex));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_scalar_index_failpoint (id, email) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT email FROM write_scalar_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_scalar_index_failpoint (id, email) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT email FROM write_scalar_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_time_series_index_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_time_series_index_failpoint");
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
                "CREATE TABLE write_time_series_index_failpoint (id INT PRIMARY KEY, tenant TEXT, event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_time_series_index_failpoint_ts_idx ON write_time_series_index_failpoint USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::TimeSeriesIndex));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_time_series_index_failpoint (id, tenant, event_at) VALUES (1, 'acme', '2026-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_time_series_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_time_series_index_failpoint (id, tenant, event_at) VALUES (1, 'acme', '2026-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT tenant FROM write_time_series_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("acme".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_graph_adjacency_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_graph_adjacency_failpoint");
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
                "CREATE GRAPH social_graph_failpoint (NODES (label TEXT), EDGES (source TEXT))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob')",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::GraphAdjacency));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM social_graph_failpoint_edges WHERE edge_id = 'e1'",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT source FROM social_graph_failpoint_edges WHERE edge_id = 'e1'",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("direct".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_normalized_vector_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_normalized_vector_failpoint");
    {
        let mut config = CassieRuntimeConfig::from_env().unwrap();
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "rollback-test".to_string(),
            dimensions: 3,
        });
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE write_normalized_vector_failpoint (id INT PRIMARY KEY, content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_normalized_vector_failpoint_idx ON write_normalized_vector_failpoint USING vector (embedding) WITH (source_field = content, index_type = hnsw)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::NormalizedVector));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_normalized_vector_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_normalized_vector_failpoint",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(
            failed.to_string().contains("injected test failure"),
            "{failed}"
        );

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_normalized_vector_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT content FROM write_normalized_vector_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn should_not_persist_document_when_vector_state_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_vector_state_failpoint");
    {
        let mut config = CassieRuntimeConfig::from_env().unwrap();
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "rollback-test".to_string(),
            dimensions: 3,
        });
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE write_vector_state_failpoint (id INT PRIMARY KEY, content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_vector_state_failpoint_idx ON write_vector_state_failpoint USING vector (embedding) WITH (source_field = content, index_type = hnsw)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_vector_state_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_vector_state_failpoint",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(
            failed.to_string().contains("injected test failure"),
            "{failed}"
        );

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_vector_state_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT content FROM write_vector_state_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
