// Consolidated integration suite: sql_mutations.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/copy_transaction_boundaries.rs.
mod copy_transaction_boundaries {
    use cassie::app::{Cassie, CassieSession};
    use cassie::runtime::QueryCancellationHandle;
    use cassie::sql::ast::{CopyFormat, CopyStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn with_copy_table(test_name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
        use_local_storage();
        let path = data_dir(test_name);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE copy_boundary_rows (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create table");
        test(&cassie, &session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    fn copy_statement() -> CopyStatement {
        CopyStatement {
            table: "copy_boundary_rows".to_string(),
            columns: vec!["id".to_string(), "title".to_string()],
            format: CopyFormat::Csv,
            header: false,
        }
    }

    fn selected_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(
                session,
                "SELECT title FROM copy_boundary_rows ORDER BY title",
                vec![],
            )
            .expect("select rows")
            .rows
    }

    #[test]
    fn should_rollback_copy_from_stdin_given_a_malformed_csv_row() {
        // Arrange
        with_copy_table("copy-malformed-row", |cassie, session| {
            let statement = copy_statement();

            // Act
            let result =
                cassie.copy_from_csv_stdin(session, &statement, b"1,alpha\n2,\"unterminated\n");

            // Assert
            assert!(result.is_err());
            assert!(selected_rows(cassie, session).is_empty());
        });
    }

    #[test]
    fn should_preserve_prior_rows_given_a_later_copy_row_failure() {
        // Arrange
        with_copy_table("copy-later-row-failure", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO copy_boundary_rows (id, title) VALUES (1, 'existing')",
                    vec![],
                )
                .expect("seed existing row");
            let statement = copy_statement();

            // Act
            let result = cassie.copy_from_csv_stdin(
                session,
                &statement,
                b"2,staged-before-error\nnot-an-integer,invalid\n",
            );

            // Assert
            assert!(result.is_err());
            assert_eq!(
                selected_rows(cassie, session),
                vec![vec![Value::String("existing".into())]]
            );
        });
    }

    #[test]
    fn should_reject_copy_inside_a_failed_transaction() {
        // Arrange
        with_copy_table("copy-failed-transaction", |cassie, session| {
            cassie
                .execute_sql(session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .execute_sql(session, "SELECT 1 / 0", vec![])
                .expect_err("fail transaction");

            // Act
            let result = cassie.copy_from_csv_stdin(session, &copy_statement(), b"1,blocked\n");

            // Assert
            assert!(result.is_err());
            assert_eq!(session.transaction_status(), "failed");
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback");
        });
    }

    #[test]
    fn should_require_rollback_after_copy_failure() {
        // Arrange
        with_copy_table("copy-requires-rollback", |cassie, session| {
            cassie
                .execute_sql(session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .copy_from_csv_stdin(session, &copy_statement(), b"invalid,blocked\n")
                .expect_err("copy failure");

            // Act
            let blocked = cassie.execute_sql(session, "SELECT 1", vec![]);
            let rollback = cassie.execute_sql(session, "ROLLBACK", vec![]);
            let recovered = cassie.execute_sql(session, "SELECT 1", vec![]);

            // Assert
            assert!(blocked.is_err());
            assert!(rollback.is_ok());
            assert!(recovered.is_ok());
        });
    }

    #[test]
    fn should_cancel_copy_before_partial_commit() {
        // Arrange
        with_copy_table("copy-cancel-before-commit", |cassie, session| {
            let cancellation = QueryCancellationHandle::new();
            cancellation.cancel();

            // Act
            let result = cassie.copy_from_csv_stdin_with_cancellation(
                session,
                &copy_statement(),
                b"1,first\n2,second\n",
                &cancellation,
            );

            // Assert
            assert!(result.is_err());
            assert!(selected_rows(cassie, session).is_empty());
        });
    }
}

// Formerly tests/dml_statement_atomicity.rs.
mod dml_statement_atomicity {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::runtime::QueryCancellationHandle;
    use cassie::types::Value;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn data_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-dml-atomicity-{label}-{}", Uuid::new_v4()))
    }

    fn database(label: &str) -> (Cassie, CassieSession, PathBuf) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("select rows")
            .rows
    }

    #[test]
    fn should_leave_zero_rows_given_later_unique_failure_when_inserting_multiple_rows() {
        // Arrange
        let (cassie, session, path) = database("insert-unique");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_unique_insert (row_key INT PRIMARY KEY, email TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");

        // Act
        let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_unique_insert (row_key, email) VALUES (1, 'same@example.com'), (2, 'same@example.com')",
        vec![],
    );

        // Assert
        assert!(
            result.is_err(),
            "later duplicate should reject the statement"
        );
        assert!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, email FROM atomic_unique_insert ORDER BY row_key"
            )
            .is_empty(),
            "a failed statement must not retain its earlier source rows"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_zero_rows_given_later_check_failure_when_inserting_multiple_rows() {
        // Arrange
        let (cassie, session, path) = database("insert-check");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_check_insert (row_key INT PRIMARY KEY, score INT CHECK (score >= 0))",
            vec![],
        )
        .expect("create table");

        // Act
        let result = cassie.execute_sql(
            &session,
            "INSERT INTO atomic_check_insert (row_key, score) VALUES (1, 10), (2, -1)",
            vec![],
        );

        // Assert
        assert!(
            result.is_err(),
            "later check failure should reject the statement"
        );
        assert!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, score FROM atomic_check_insert ORDER BY row_key"
            )
            .is_empty(),
            "a failed statement must not retain its earlier source rows"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_zero_child_rows_given_later_foreign_key_failure_when_inserting_multiple_rows() {
        // Arrange
        let (cassie, session, path) = database("insert-foreign-key");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_fk_parents (row_key INT PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_fk_children (row_key INT PRIMARY KEY, parent_key INT REFERENCES atomic_fk_parents(row_key))",
            vec![],
        )
        .expect("create child table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_fk_parents (row_key) VALUES (1)",
                vec![],
            )
            .expect("seed parent");

        // Act
        let result = cassie.execute_sql(
            &session,
            "INSERT INTO atomic_fk_children (row_key, parent_key) VALUES (1, 1), (2, 999)",
            vec![],
        );

        // Assert
        assert!(
            result.is_err(),
            "later foreign key failure should reject the statement"
        );
        assert!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, parent_key FROM atomic_fk_children ORDER BY row_key"
            )
            .is_empty(),
            "a failed statement must not retain its earlier source rows"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_all_rows_unchanged_given_multi_row_update_unique_conflict() {
        // Arrange
        let (cassie, session, path) = database("update-unique");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_unique_update (row_key INT PRIMARY KEY, email TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_unique_update (row_key, email) VALUES (1, 'one@example.com'), (2, 'two@example.com')",
            vec![],
        )
        .expect("seed rows");

        // Act
        let result = cassie.execute_sql(
            &session,
            "UPDATE atomic_unique_update SET email = 'same@example.com'",
            vec![],
        );

        // Assert
        assert!(result.is_err(), "unique conflict should reject the update");
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, email FROM atomic_unique_update ORDER BY row_key"
            ),
            vec![
                vec![
                    Value::Int64(1),
                    Value::String("one@example.com".to_string())
                ],
                vec![
                    Value::Int64(2),
                    Value::String("two@example.com".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_restore_all_collections_given_delete_cascade_followed_by_restrict_failure() {
        // Arrange
        let (cassie, session, path) = database("delete-cascade-failure");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_delete_parents (row_key INT PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_delete_a_cascade (row_key INT PRIMARY KEY, parent_key INT, CONSTRAINT atomic_delete_a_cascade_fk FOREIGN KEY (parent_key) REFERENCES atomic_delete_parents(row_key) ON DELETE CASCADE)",
            vec![],
        )
        .expect("create cascade child table");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_delete_z_restrict (row_key INT PRIMARY KEY, parent_key INT REFERENCES atomic_delete_parents(row_key))",
            vec![],
        )
        .expect("create restricting child table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_delete_parents (row_key) VALUES (1)",
                vec![],
            )
            .expect("seed parent");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_delete_a_cascade (row_key, parent_key) VALUES (10, 1)",
                vec![],
            )
            .expect("seed cascade child");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_delete_z_restrict (row_key, parent_key) VALUES (20, 1)",
                vec![],
            )
            .expect("seed restricting child");

        // Act
        let result = cassie.execute_sql(
            &session,
            "DELETE FROM atomic_delete_parents WHERE row_key = 1",
            vec![],
        );

        // Assert
        assert!(
            result.is_err(),
            "restricting child should reject the delete"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key FROM atomic_delete_parents"
            ),
            vec![vec![Value::Int64(1)]]
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key FROM atomic_delete_a_cascade"
            ),
            vec![vec![Value::Int64(10)]],
            "the earlier cascade must be rolled back"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key FROM atomic_delete_z_restrict"
            ),
            vec![vec![Value::Int64(20)]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_restore_all_collections_given_update_cascade_constraint_failure() {
        // Arrange
        let (cassie, session, path) = database("update-cascade-failure");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_update_parents (row_key INT PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_update_children (row_key INT PRIMARY KEY, parent_key INT CHECK (parent_key < 2), CONSTRAINT atomic_update_children_fk FOREIGN KEY (parent_key) REFERENCES atomic_update_parents(row_key) ON UPDATE CASCADE)",
            vec![],
        )
        .expect("create child table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_update_parents (row_key) VALUES (1)",
                vec![],
            )
            .expect("seed parent");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_update_children (row_key, parent_key) VALUES (10, 1)",
                vec![],
            )
            .expect("seed child");

        // Act
        let result = cassie.execute_sql(
            &session,
            "UPDATE atomic_update_parents SET row_key = 2 WHERE row_key = 1",
            vec![],
        );

        // Assert
        assert!(
            result.is_err(),
            "cascaded child check failure should reject the update"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key FROM atomic_update_parents"
            ),
            vec![vec![Value::Int64(1)]],
            "the parent update must be rolled back"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, parent_key FROM atomic_update_children"
            ),
            vec![vec![Value::Int64(10), Value::Int64(1)]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_prior_transaction_work_given_failed_statement_rolled_back_to_savepoint() {
        // Arrange
        let (cassie, session, path) = database("transaction-savepoint");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_transaction_rows (row_key INT PRIMARY KEY, score INT CHECK (score >= 0))",
            vec![],
        )
        .expect("create table");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_transaction_rows (row_key, score) VALUES (1, 10)",
                vec![],
            )
            .expect("stage earlier statement");
        cassie
            .execute_sql(&session, "SAVEPOINT before_failure", vec![])
            .expect("create savepoint");

        // Act
        let failed = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_transaction_rows (row_key, score) VALUES (2, 20), (3, -1) RETURNING row_key",
        vec![],
    );
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT before_failure", vec![])
            .expect("recover failed transaction");
        let visible_after_recovery = rows(
            &cassie,
            &session,
            "SELECT row_key, score FROM atomic_transaction_rows ORDER BY row_key",
        );
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("commit earlier work");

        // Assert
        assert!(
            failed.is_err(),
            "later check failure should reject the statement"
        );
        assert_eq!(
            visible_after_recovery,
            vec![vec![Value::Int64(1), Value::Int64(10)]],
            "savepoint recovery must preserve prior work without partial failed-statement writes"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, score FROM atomic_transaction_rows ORDER BY row_key"
            ),
            visible_after_recovery
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rollback_prior_source_rows_given_later_on_conflict_update_failure() {
        // Arrange
        let (cassie, session, path) = database("upsert-atomicity");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_upsert_rows (row_key INT PRIMARY KEY, email TEXT UNIQUE, note TEXT)",
            vec![],
        )
        .expect("create table");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_upsert_rows (row_key, email, note) VALUES (1, 'one@example.com', 'original')",
            vec![],
        )
        .expect("seed conflict row");

        // Act
        let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_upsert_rows (row_key, email, note) VALUES (2, 'two@example.com', 'inserted'), (1, 'two@example.com', 'updated') ON CONFLICT (row_key) DO UPDATE SET email = excluded.email, note = excluded.note RETURNING row_key, email, note",
        vec![],
    );

        // Assert
        assert!(
            result.is_err(),
            "unique failure in conflict update should reject the statement"
        );
        assert_eq!(
            rows(
                &cassie,
                &session,
                "SELECT row_key, email, note FROM atomic_upsert_rows ORDER BY row_key"
            ),
            vec![vec![
                Value::Int64(1),
                Value::String("one@example.com".to_string()),
                Value::String("original".to_string())
            ]],
            "the earlier source-row insert must be rolled back"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_source_order_given_multi_row_on_conflict_returning() {
        // Arrange
        let (cassie, session, path) = database("upsert-returning-order");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_upsert_order (row_key INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO atomic_upsert_order (row_key, title) VALUES (2, 'old')",
                vec![],
            )
            .expect("seed conflict row");

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_upsert_order (row_key, title) VALUES (3, 'third'), (2, 'updated'), (1, 'first') ON CONFLICT (row_key) DO UPDATE SET title = excluded.title RETURNING row_key, title",
            vec![],
        )
        .expect("execute upsert");

        // Assert
        assert_eq!(result.command, "INSERT 0 3");
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(3), Value::String("third".to_string())],
                vec![Value::Int64(2), Value::String("updated".to_string())],
                vec![Value::Int64(1), Value::String("first".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_zero_rows_given_cancelled_multi_row_insert_before_execution() {
        // Arrange
        let (cassie, session, path) = database("cancel-before-execution");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE atomic_cancelled_insert (row_key INT PRIMARY KEY)",
                vec![],
            )
            .expect("create table");
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        let error = cassie
        .execute_sql_with_cancellation(
            &session,
            "INSERT INTO atomic_cancelled_insert (row_key) VALUES (1), (2), (3) RETURNING row_key",
            vec![],
            &cancellation,
        )
        .expect_err("cancelled mutation should fail");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert!(
            rows(
                &cassie,
                &session,
                "SELECT row_key FROM atomic_cancelled_insert"
            )
            .is_empty(),
            "a cancelled mutation must not write rows"
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/foreign_key_concurrency.rs.
mod foreign_key_concurrency {
    use cassie::app::Cassie;

    use super::support_sql as support;

    #[test]
    fn should_not_commit_an_orphaned_child_during_concurrent_parent_delete() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("foreign_key_concurrent_parent_delete");
        let cassie = std::sync::Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_race_parents (id INT PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_race_children (parent_id INT REFERENCES fk_race_parents(id))",
                vec![],
            )
            .expect("create child table");

        for attempt in 0..32 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO fk_race_parents (id) VALUES (1)",
                    vec![],
                )
                .expect("insert parent");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let insert_cassie = std::sync::Arc::clone(&cassie);
            let insert_barrier = std::sync::Arc::clone(&barrier);
            let child_insert = std::thread::spawn(move || {
                let session = insert_cassie.create_session("tester", None);
                insert_barrier.wait();
                insert_cassie.execute_sql(
                    &session,
                    "INSERT INTO fk_race_children (parent_id) VALUES (1)",
                    vec![],
                )
            });
            let delete_cassie = std::sync::Arc::clone(&cassie);
            let delete_barrier = std::sync::Arc::clone(&barrier);
            let parent_delete = std::thread::spawn(move || {
                let session = delete_cassie.create_session("tester", None);
                delete_barrier.wait();
                delete_cassie.execute_sql(
                    &session,
                    "DELETE FROM fk_race_parents WHERE id = 1",
                    vec![],
                )
            });
            barrier.wait();

            // Act
            let child_result = child_insert.join().expect("child insert worker completed");
            let parent_result = parent_delete
                .join()
                .expect("parent delete worker completed");
            let parent_collection = support::canonical_test_collection(&cassie, "fk_race_parents");
            let child_collection = support::canonical_test_collection(&cassie, "fk_race_children");
            let parents = cassie
                .midge
                .scan_documents(&parent_collection)
                .expect("scan parents");
            let children = cassie
                .midge
                .scan_documents(&child_collection)
                .expect("scan children");

            // Assert
            let no_rows_remain = parents.is_empty() && children.is_empty();
            let parent_and_child_remain = parents.len() == 1 && children.len() == 1;
            assert!(
            no_rows_remain || parent_and_child_remain,
            "attempt {attempt} committed an orphaned child: child={child_result:?}, parent={parent_result:?}"
        );

            cassie
                .execute_sql(&session, "DELETE FROM fk_race_children", vec![])
                .expect("clear children");
            cassie
                .execute_sql(&session, "DELETE FROM fk_race_parents", vec![])
                .expect("clear parents");
        }

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_constraints.rs.
mod integration_sql_constraints {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_enforce_constraints_during_ingest() {
        // Arrange
        use_local_storage();
        let path = data_dir("constraints_ingest");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let create = cassie
            .execute_sql(
                &session,
                "CREATE TABLE constraint_docs (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE, status TEXT DEFAULT 'pending', score INT CHECK (score >= 18))",
                vec![],
            )

.unwrap();
        let collection = canonical_test_collection(&cassie, "constraint_docs");

        let first = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 1, "email": "a@example.com", "score": 25}),
            )
            .unwrap();
        let missing_not_null = cassie
            .ingest_document(&collection, serde_json::json!({"id": 2, "score": 20}));
        let duplicate = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 3, "email": "a@example.com", "score": 19}),
            );
        let rejected_check = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 4, "email": "b@example.com", "score": 17}),
            );

        let inserted = cassie
            .midge
            .get_document(&collection, &first)

            .unwrap()
            .expect("document inserted");

        // Assert
        assert_eq!(create.command, "CREATE TABLE");
        assert_eq!(
            inserted.payload.get("status").expect("status is defaulted"),
            &serde_json::Value::String("pending".to_string())
        );
        assert!(missing_not_null.is_err());
        assert!(missing_not_null
            .unwrap_err()
            .to_string()
            .contains("cannot be null"));
        assert!(duplicate.is_err());
        assert!(duplicate
            .unwrap_err()
            .to_string()
            .contains("unique constraint"));
        assert!(rejected_check.is_err());
        assert!(rejected_check
            .unwrap_err()
            .to_string()
            .contains("check constraint"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_hydrate_collection_constraints_on_startup() {
        // Arrange
        use_local_storage();
        let path = data_dir("constraints_hydrate");
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
                "CREATE TABLE hydrated_constraints (id INT, email TEXT NOT NULL UNIQUE, score INT CHECK (score >= 0))",
                vec![],
            )

.unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        let constraints = restarted.catalog.get_constraints("hydrated_constraints");
        // Assert
        assert_eq!(constraints.len(), 2);
        assert!(constraints.iter().any(|constraint| constraint.not_null));
        assert!(constraints.iter().any(|constraint| constraint.unique));
        assert!(constraints.iter().any(|constraint| constraint.check.is_some()));
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_insert_when_primary_key_is_duplicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("primary_key_duplicate");
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
                    "CREATE TABLE primary_key_duplicate (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'beta')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted
                .unwrap_err()
                .to_string()
                .contains("unique constraint failed for 'id'"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_when_primary_key_is_null() {
        // Arrange
        use_local_storage();
        let path = data_dir("primary_key_null");
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
                    "CREATE TABLE primary_key_null (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO primary_key_null (id, title) VALUES (NULL, 'alpha')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_when_unique_value_is_duplicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("unique_insert_duplicate");
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
                    "CREATE TABLE unique_insert_duplicate (email TEXT UNIQUE)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted
                .unwrap_err()
                .to_string()
                .contains("unique constraint failed for 'email'"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_update_when_unique_value_conflicts() {
        // Arrange
        use_local_storage();
        let path = data_dir("unique_update_conflict");
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
                "CREATE TABLE unique_update_conflict (email TEXT UNIQUE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            );

        // Assert
        assert!(updated.is_err());
        assert!(updated
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'email'"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_allow_only_one_concurrent_unique_insert_to_commit() {
        // Arrange
        use_local_storage();
        let path = data_dir("unique_concurrent_insert");
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
                    "CREATE TABLE unique_concurrent_insert (email TEXT UNIQUE)",
                    vec![],
                )
                .unwrap();

            let cassie = std::sync::Arc::new(cassie);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
            let workers = (0..2)
                .map(|_| {
                    let cassie = std::sync::Arc::clone(&cassie);
                    let barrier = std::sync::Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        let session = cassie.create_session("tester", None);
                        barrier.wait();
                        cassie.execute_sql(
                        &session,
                        "INSERT INTO unique_concurrent_insert (email) VALUES ('same@example.com')",
                        vec![],
                    )
                    })
                })
                .collect::<Vec<_>>();
            barrier.wait();

            // Act
            let results = workers
                .into_iter()
                .map(|worker| worker.join().expect("worker completed"))
                .collect::<Vec<_>>();

            // Assert
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
            assert!(results.iter().any(|result| {
                result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.to_string().contains("unique constraint"))
            }));
            let selected = cassie
                .execute_sql(
                    &cassie.create_session("tester", None),
                    "SELECT email FROM unique_concurrent_insert",
                    vec![],
                )
                .unwrap();
            assert_eq!(selected.rows.len(), 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_when_unique_index_value_is_duplicate() {
        // Arrange
        use_local_storage();
        let path = data_dir("unique_index_insert_duplicate");
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
                "CREATE TABLE unique_index_insert_duplicate (email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_email_idx ON unique_index_insert_duplicate USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "unique_index_insert_duplicate");
        let index = cassie
            .catalog
            .get_index(&collection, "unique_index_email_idx")
            .expect("index metadata");

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains(&format!("unique index '{}' failed", index.name)));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_update_when_unique_index_value_conflicts() {
        // Arrange
        use_local_storage();
        let path = data_dir("unique_index_update_conflict");
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
                "CREATE TABLE unique_index_update_conflict (email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_update_email_idx ON unique_index_update_conflict USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "unique_index_update_conflict");
        let index = cassie
            .catalog
            .get_index(&collection, "unique_index_update_email_idx")
            .expect("index metadata");

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_index_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            );

        // Assert
        assert!(updated.is_err());
        assert!(updated
            .unwrap_err()
            .to_string()
            .contains(&format!("unique index '{}' failed", index.name)));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_insert_when_check_constraint_fails() {
        // Arrange
        use_local_storage();
        let path = data_dir("check_insert_failure");
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
                    "CREATE TABLE check_insert_failure (score INT CHECK (score >= 18))",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO check_insert_failure (score) VALUES (17)",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted
                .unwrap_err()
                .to_string()
                .contains("check constraint failed"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_update_when_check_constraint_fails() {
        // Arrange
        use_local_storage();
        let path = data_dir("check_update_failure");
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
                    "CREATE TABLE check_update_failure (score INT CHECK (score >= 18))",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO check_update_failure (score) VALUES (20)",
                    vec![],
                )
                .unwrap();

            // Act
            let updated = cassie.execute_sql(
                &session,
                "UPDATE check_update_failure SET score = 17",
                vec![],
            );

            // Assert
            assert!(updated.is_err());
            let message = updated.unwrap_err().to_string();
            assert!(
                message.contains("check constraint failed"),
                "expected check constraint error, got {message}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_insert_on_conflict_do_update() {
        // Arrange
        use_local_storage();
        let path = data_dir("on_conflict_do_update");
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
                "CREATE TABLE on_conflict_do_update (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_update (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_update (id, title) VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title RETURNING title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "INSERT 0 1");
        assert_eq!(result.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_insert_on_conflict_do_nothing() {
        // Arrange
        use_local_storage();
        let path = data_dir("on_conflict_do_nothing");
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
                "CREATE TABLE on_conflict_do_nothing (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_nothing (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_nothing (id, title) VALUES (1, 'beta') ON CONFLICT DO NOTHING",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM on_conflict_do_nothing ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "INSERT 0 0");
        assert_eq!(rows.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_delete.rs.
mod integration_sql_delete {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_delete_where_returning_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("delete_where_returning");
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
                    "CREATE TABLE delete_where_returning (title TEXT, status TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO delete_where_returning (title, status) VALUES ('alpha', 'old')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO delete_where_returning (title, status) VALUES ('beta', 'old')",
                    vec![],
                )
                .unwrap();

            // Act
            let deleted = cassie
                .execute_sql(
                    &session,
                    "DELETE FROM delete_where_returning WHERE title = 'alpha' RETURNING _id, title",
                    vec![],
                )
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM delete_where_returning ORDER BY title ASC",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(deleted.command, "DELETE 1");
            assert_eq!(deleted.rows.len(), 1);
            assert!(matches!(&deleted.rows[0][0], Value::String(id) if !id.is_empty()));
            assert_eq!(deleted.rows[0][1], Value::String("alpha".to_string()));
            assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_delete_returning_scalar_function() {
        // Arrange
        use_local_storage();
        let path = data_dir("delete_returning_function");
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
                    "CREATE TABLE delete_returning_function (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO delete_returning_function (title) VALUES ('ALPHA')",
                    vec![],
                )
                .unwrap();

            // Act
            let deleted = cassie
                .execute_sql(
                    &session,
                    "DELETE FROM delete_returning_function RETURNING lower(title) AS normalized",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(deleted.columns[0].name, "normalized");
            assert_eq!(deleted.rows, vec![vec![Value::String("alpha".to_string())]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_zero_rows_for_delete_without_matches() {
        // Arrange
        use_local_storage();
        let path = data_dir("delete_no_match");
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
                    "CREATE TABLE delete_no_match (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO delete_no_match (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let deleted = cassie
                .execute_sql(
                    &session,
                    "DELETE FROM delete_no_match WHERE title = 'missing' RETURNING title",
                    vec![],
                )
                .unwrap();
            let selected = cassie
                .execute_sql(&session, "SELECT title FROM delete_no_match", vec![])
                .unwrap();

            // Assert
            assert_eq!(deleted.command, "DELETE 0");
            assert!(deleted.rows.is_empty());
            assert_eq!(selected.rows.len(), 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_ignore_legacy_fallback_key_for_sql_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("delete_legacy_cleanup");
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
                    "CREATE TABLE delete_legacy_cleanup (title TEXT)",
                    vec![],
                )
                .unwrap();
            let collection = canonical_test_collection(&cassie, "delete_legacy_cleanup");
            let inserted = cassie
                .execute_sql(
                    &session,
                    "INSERT INTO delete_legacy_cleanup (title) VALUES ('alpha') RETURNING _id",
                    vec![],
                )
                .unwrap();
            let row_id = match &inserted.rows[0][0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected row id"),
            };
            put_legacy_document(
                &cassie,
                &collection,
                &row_id,
                &serde_json::json!({"title": "stale"}),
            );

            // Act
            cassie
                .execute_sql(
                    &session,
                    "DELETE FROM delete_legacy_cleanup WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let deleted = cassie.midge.get_document(&collection, &row_id).unwrap();

            // Assert
            assert!(deleted.is_none());

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_drop_constraint.rs.
mod integration_sql_drop_constraint {
    use cassie::app::Cassie;

    use super::support_sql as support;

    #[test]
    fn should_preserve_other_field_rules_when_named_primary_key_is_dropped() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-primary-key");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_primary (code TEXT, CONSTRAINT code_pk PRIMARY KEY (code), CONSTRAINT code_check CHECK (code <> ''))",
            vec![],
        )
        .expect("create table");

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE drop_primary DROP CONSTRAINT code_pk",
                vec![],
            )
            .expect("drop primary key");

        // Assert
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_primary (code) VALUES (NULL)",
                vec![],
            )
            .expect("primary key rule removed");
        let check_error = cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_primary (code) VALUES ('')",
                vec![],
            )
            .expect_err("check remains");
        assert!(check_error.to_string().contains("check constraint"));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_allow_duplicate_values_when_named_unique_constraint_is_dropped() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-unique");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE drop_unique (email TEXT, CONSTRAINT email_key UNIQUE (email))",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_unique (email) VALUES ('same@example.com')",
                vec![],
            )
            .expect("insert first row");

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE drop_unique DROP CONSTRAINT email_key",
                vec![],
            )
            .expect("drop unique constraint");

        // Assert
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_unique (email) VALUES ('same@example.com')",
                vec![],
            )
            .expect("duplicate accepted");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_remove_named_check_plus_foreign_key_constraints() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-check-foreign-key");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE drop_parents (id TEXT PRIMARY KEY)",
                vec![],
            )
            .expect("create parents");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_children (parent_id TEXT, score INT, CONSTRAINT score_check CHECK (score > 0), CONSTRAINT parent_fk FOREIGN KEY (parent_id) REFERENCES drop_parents(id))",
            vec![],
        )
        .expect("create children");

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE drop_children DROP CONSTRAINT score_check",
                vec![],
            )
            .expect("drop check");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE drop_children DROP CONSTRAINT parent_fk",
                vec![],
            )
            .expect("drop foreign key");

        // Assert
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_children (parent_id, score) VALUES ('missing', -1)",
                vec![],
            )
            .expect("removed constraints no longer enforce writes");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_dropping_unique_constraint_with_live_foreign_key_dependency() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-live-dependency");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE dependency_parents (email TEXT, CONSTRAINT dependency_email_key UNIQUE (email))",
            vec![],
        )
        .expect("create parents");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE dependency_children (email TEXT REFERENCES dependency_parents(email))",
            vec![],
        )
        .expect("create children");

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "ALTER TABLE dependency_parents DROP CONSTRAINT dependency_email_key",
                vec![],
            )
            .expect_err("dependency blocks drop");

        // Assert
        assert!(error.to_string().contains("depends on it"));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_hydrate_dropped_constraint_state_after_restart() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-constraint-restart");
        {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
            .execute_sql(
                &session,
                "CREATE TABLE restart_drop_unique (email TEXT, CONSTRAINT restart_email_key UNIQUE (email))",
                vec![],
            )
            .expect("create table");
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE restart_drop_unique DROP CONSTRAINT restart_email_key",
                    vec![],
                )
                .expect("drop constraint");
        }

        // Act
        let cassie = Cassie::new_with_data_dir(&path).expect("reopened cassie");
        cassie.startup().expect("restart");
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "INSERT INTO restart_drop_unique (email) VALUES ('same@example.com'), ('same@example.com')",
            vec![],
        )
        .expect("duplicates accepted after restart");

        // Assert
        let rows = cassie
            .execute_sql(&session, "SELECT email FROM restart_drop_unique", vec![])
            .expect("read rows");
        assert_eq!(rows.rows.len(), 2);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_drop_constraint_if_exists_for_missing_name() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop-constraint-if-exists");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE drop_if_exists (id TEXT)", vec![])
            .expect("create table");

        // Act
        let result = cassie.execute_sql(
            &session,
            "ALTER TABLE drop_if_exists DROP CONSTRAINT IF EXISTS missing_key",
            vec![],
        );

        // Assert
        result.expect("missing constraint ignored");

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/integration_sql_foreign_keys.rs.
mod integration_sql_foreign_keys {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;
    #[test]
    fn should_reject_insert_when_foreign_key_parent_is_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_missing_parent");
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
                    "CREATE TABLE fk_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_children (parent_id INT REFERENCES fk_parents(id), title TEXT)",
                vec![],
            )
            .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO fk_parents (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO fk_children (parent_id, title) VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            let missing_parent = cassie.execute_sql(
                &session,
                "INSERT INTO fk_children (parent_id, title) VALUES (2, 'missing')",
                vec![],
            );

            // Assert
            assert!(missing_parent.is_err());
            assert!(missing_parent
                .unwrap_err()
                .to_string()
                .contains("foreign key constraint"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_parent_mutation_when_foreign_key_children_exist() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_referenced_parent");
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
                "CREATE TABLE fk_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_children (parent_id INT REFERENCES fk_parents(id), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_children (parent_id, title) VALUES (1, 'child')",
                vec![],
            )
            .unwrap();

        // Act
        let delete_parent = cassie.execute_sql(
            &session,
            "DELETE FROM fk_parents WHERE title = 'alpha'",
            vec![],
        );
        let update_parent = cassie.execute_sql(
            &session,
            "UPDATE fk_parents SET id = 2 WHERE title = 'alpha'",
            vec![],
        );
        let constraints = cassie
            .execute_sql(
                &session,
                "SELECT constraint_type FROM information_schema.table_constraints WHERE table_name = 'fk_children' ORDER BY constraint_type",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(delete_parent.is_err());
        assert!(delete_parent
            .unwrap_err()
            .to_string()
            .contains("foreign key constraint"));
        assert!(update_parent.is_err());
        assert!(update_parent
            .unwrap_err()
            .to_string()
            .contains("foreign key constraint"));
        assert!(constraints.rows.iter().any(|row| {
            row == &vec![Value::String("FOREIGN KEY".to_string())]
        }));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_foreign_key_delete_actions() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_delete_actions");
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
                "CREATE TABLE fk_delete_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_delete_cascade_children (parent_id INT, title TEXT, CONSTRAINT fk_delete_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_delete_parents(id) ON DELETE CASCADE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_delete_null_children (parent_id INT, title TEXT, CONSTRAINT fk_delete_null_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_delete_parents(id) ON DELETE SET NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_cascade_children (parent_id, title) VALUES (1, 'cascade')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_delete_null_children (parent_id, title) VALUES (1, 'nullable')",
                vec![],
            )
            .unwrap();

        // Act
        let delete_parent = cassie
            .execute_sql(
                &session,
                "DELETE FROM fk_delete_parents WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let cascade_children = cassie
            .execute_sql(
                &session,
                "SELECT title FROM fk_delete_cascade_children",
                vec![],
            )
            .unwrap();
        let null_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_delete_null_children",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(delete_parent.command, "DELETE 1");
        assert!(cascade_children.rows.is_empty());
        assert_eq!(
            null_children.rows,
            vec![vec![
                Value::Null,
                Value::String("nullable".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_foreign_key_update_actions() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_update_actions");
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
                "CREATE TABLE fk_update_parents (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_update_cascade_children (parent_id INT, title TEXT, CONSTRAINT fk_update_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_update_parents(id) ON UPDATE CASCADE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_update_null_children (parent_id INT, title TEXT, CONSTRAINT fk_update_null_children_fkey FOREIGN KEY (parent_id) REFERENCES fk_update_parents(id) ON UPDATE SET NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_parents (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_cascade_children (parent_id, title) VALUES (1, 'cascade')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO fk_update_null_children (parent_id, title) VALUES (1, 'nullable')",
                vec![],
            )
            .unwrap();

        // Act
        let update_parent = cassie
            .execute_sql(
                &session,
                "UPDATE fk_update_parents SET id = 2 WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let cascade_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_update_cascade_children",
                vec![],
            )
            .unwrap();
        let null_children = cassie
            .execute_sql(
                &session,
                "SELECT parent_id, title FROM fk_update_null_children",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(update_parent.command, "UPDATE 1");
        assert_eq!(
            cascade_children.rows,
            vec![vec![
                Value::Int64(2),
                Value::String("cascade".to_string())
            ]]
        );
        assert_eq!(
            null_children.rows,
            vec![vec![
                Value::Null,
                Value::String("nullable".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_idempotent_ddl.rs.
mod integration_sql_idempotent_ddl {
    use cassie::app::Cassie;
    use cassie::types::Value;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-idempotent-ddl-{name}-{}", Uuid::new_v4()))
    }

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    #[test]
    fn should_preserve_table_given_mismatched_if_not_exists_definition() {
        // Arrange
        use_local_storage();
        let path = data_dir("table");
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
                    "CREATE TABLE stable_docs (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO stable_docs VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            let epoch = cassie.midge.schema_epoch().unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE IF NOT EXISTS stable_docs (different VECTOR(7) NOT NULL)",
                    vec![],
                )
                .unwrap();
            let rows = cassie
                .execute_sql(&session, "SELECT title FROM stable_docs", vec![])
                .unwrap();

            // Assert
            assert_eq!(result.command, "CREATE TABLE");
            assert_eq!(cassie.midge.schema_epoch().unwrap(), epoch);
            assert_eq!(rows.rows, vec![vec![Value::String("alpha".into())]]);
            assert!(cassie
                .execute_sql(&session, "CREATE TABLE stable_docs (id INT)", vec![])
                .is_err());
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_preserve_idempotent_ddl_given_repeated_create_index_if_not_exists() {
        // Arrange
        use_local_storage();
        let path = data_dir("index");
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
                    "CREATE TABLE indexed_docs (id INT, title TEXT, score INT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE UNIQUE INDEX stable_idx ON indexed_docs (title)",
                    vec![],
                )
                .unwrap();
            let collection = cassie
                .catalog
                .get_schema("indexed_docs")
                .unwrap()
                .collection;
            let before = cassie.catalog.get_index(&collection, "stable_idx").unwrap();
            let epoch = cassie.midge.schema_epoch().unwrap();

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX IF NOT EXISTS stable_idx ON indexed_docs (score)",
                    vec![],
                )
                .unwrap();
            let after = cassie.catalog.get_index(&collection, "stable_idx").unwrap();

            // Assert
            assert_eq!(result.command, "CREATE INDEX");
            assert_eq!(cassie.midge.schema_epoch().unwrap(), epoch);
            assert_eq!(after.fields, before.fields);
            assert_eq!(after.unique, before.unique);
            assert!(cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX stable_idx ON indexed_docs (score)",
                    vec![]
                )
                .is_err());
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_insert_select.rs.
mod integration_sql_insert_select {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_insert_select_with_returning_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_select_returning");
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
                "CREATE TABLE insert_select_source (title TEXT, score INT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_target (name TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_source (title, score) VALUES ('banana', 2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_source (title, score) VALUES ('apple', 1)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_target (name, score) SELECT title, score FROM insert_select_source ORDER BY title ASC RETURNING name, score",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.command, "INSERT 0 2");
        assert_eq!(inserted.rows.len(), 2);
        assert_eq!(inserted.rows[0][0], Value::String("apple".to_string()));
        assert_eq!(inserted.rows[0][1], Value::Int64(1));
        assert_eq!(inserted.rows[1][0], Value::String("banana".to_string()));
        assert_eq!(inserted.rows[1][1], Value::Int64(2));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_insert_select_shape_mismatch_before_writing() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_select_shape");
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
                "CREATE TABLE insert_select_shape_source (title TEXT, body TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_shape_target (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_shape_source (title, body) VALUES ('alpha', 'first')",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_shape_target (title) SELECT title, body FROM insert_select_shape_source",
                vec![],
            );
        let target_rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM insert_select_shape_target",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("column/value counts mismatch"));
        assert!(target_rows.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_apply_default_values_for_insert_select() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_select_defaults");
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
                "CREATE TABLE insert_select_default_source (source_id INT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE insert_select_default_target (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_default_source (source_id) VALUES (1)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_select_default_target (id) SELECT source_id FROM insert_select_default_source RETURNING status",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("pending".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_insert_values.rs.
mod integration_sql_insert_values {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_insert_values_with_explicit_columns_returning_columns() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_returning");
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
                "CREATE TABLE insert_values_returning (title TEXT, body TEXT)",
                vec![],
            )

.unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_returning (title, body) VALUES ('alpha', 'first') RETURNING title, body",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "INSERT 0 1");
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.columns[1].name, "body");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][1], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_insert_values_using_table_column_order() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_table_order");
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
                    "CREATE TABLE insert_values_table_order (title TEXT, score INT)",
                    vec![],
                )
                .unwrap();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO insert_values_table_order VALUES ('alpha', 7)",
                    vec![],
                )
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title, score FROM insert_values_table_order",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows.len(), 1);
            assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));
            assert_eq!(selected.rows[0][1], Value::Int64(7));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_insert_multiple_values_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_multiple_values");
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
                "CREATE TABLE insert_multiple_values (title TEXT, score INT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_multiple_values (title, score) VALUES ('alpha', 1), ('beta', 2) RETURNING title, score",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.command, "INSERT 0 2");
        assert_eq!(
            inserted.rows,
            vec![
                vec![Value::String("alpha".to_string()), Value::Int64(1)],
                vec![Value::String("beta".to_string()), Value::Int64(2)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_return_generated_row_id_from_insert_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_id");
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
                    "CREATE TABLE insert_values_id (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie
                .execute_sql(
                    &session,
                    "INSERT INTO insert_values_id (title) VALUES ('alpha') RETURNING _id",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(inserted.columns[0].name, "_id");
            assert_eq!(inserted.rows.len(), 1);
            assert!(matches!(&inserted.rows[0][0], Value::String(id) if !id.is_empty()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_execute_insert_returning_wildcard() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_returning_wildcard");
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
                "CREATE TABLE insert_returning_wildcard (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_wildcard (title, body) VALUES ('alpha', 'first') RETURNING *",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.columns[0].name, "_id");
        assert_eq!(inserted.columns[1].name, "title");
        assert_eq!(inserted.columns[2].name, "body");
        assert!(matches!(&inserted.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(inserted.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(inserted.rows[0][2], Value::String("first".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_insert_returning_scalar_function() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_returning_function");
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
                "CREATE TABLE insert_returning_function (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_function (title) VALUES ('ALPHA') RETURNING lower(title) AS normalized",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.columns[0].name, "normalized");
        assert_eq!(
            inserted.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_insert_returning_unknown_function() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_returning_unknown_function");
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
                "CREATE TABLE insert_returning_unknown_function (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_returning_unknown_function (title) VALUES ('ALPHA') RETURNING missing_fn(title)",
                vec![],
            );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unsupported function"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_insert_values_when_not_null_constraint_fails() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_not_null");
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
                    "CREATE TABLE insert_values_not_null (title TEXT NOT NULL)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_values_not_null (title) VALUES (NULL)",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_values_when_not_null_column_is_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_missing_not_null");
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
                    "CREATE TABLE insert_values_missing_not_null (title TEXT NOT NULL, body TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_values_missing_not_null (body) VALUES ('first')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_apply_default_values_for_insert_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_defaults");
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
                "CREATE TABLE insert_values_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

.unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_defaults (id) VALUES (1) RETURNING status",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("pending".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_explicit_insert_value_when_default_exists() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_explicit_default");
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
                "CREATE TABLE insert_values_explicit_default (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_explicit_default (id, status) VALUES (1, 'done') RETURNING status",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(inserted.rows.len(), 1);
        assert_eq!(inserted.rows[0][0], Value::String("done".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_round_trip_insert_values_vector_field() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_vector_round_trip");
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
                "CREATE TABLE insert_values_vector_round_trip (doc_id TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO insert_values_vector_round_trip (doc_id, embedding) VALUES ('row-1', $1)",
                vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT embedding FROM insert_values_vector_round_trip WHERE doc_id = 'row-1'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::Vector(Vector::new(vec![1.0, 2.0, 3.0]))]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_insert_values_when_vector_dimensions_mismatch() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_vector_dimensions");
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
                    "CREATE TABLE insert_values_vector_dimensions (embedding VECTOR(2))",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_values_vector_dimensions (embedding) VALUES ($1)",
                vec![Value::Vector(Vector::new(vec![1.0]))],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted
                .unwrap_err()
                .to_string()
                .contains("expects vector(2)"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_values_with_duplicate_target_column() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_duplicate_column");
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
                    "CREATE TABLE insert_duplicate_column (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_duplicate_column (title, title) VALUES ('alpha', 'beta')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted.unwrap_err().to_string().contains("duplicated"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_insert_values_with_unknown_target_column() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_unknown_column");
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
                    "CREATE TABLE insert_unknown_column (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_unknown_column (missing) VALUES ('alpha')",
                vec![],
            );

            // Assert
            assert!(inserted.is_err());
            assert!(inserted.unwrap_err().to_string().contains("does not exist"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_store_insert_values_as_row_blobs() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_values_row_blob");
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
                    "CREATE TABLE insert_values_row_blob (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO insert_values_row_blob (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let legacy_row_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"r/insert_values_row_blob/")
                .unwrap();
            let legacy_entries = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Data, b"doc:insert_values_row_blob:")
                .unwrap();
            let selected = cassie
                .execute_sql(&session, "SELECT title FROM insert_values_row_blob", vec![])
                .unwrap();

            // Assert
            assert!(legacy_row_entries.is_empty());
            assert!(legacy_entries.is_empty());
            assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_project_missing_sparse_row_fields_as_null() {
        // Arrange
        use_local_storage();
        let path = data_dir("sparse_row_projection");
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
                    "CREATE TABLE sparse_row_projection (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sparse_row_projection (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title, body FROM sparse_row_projection",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(selected.rows.len(), 1);
            assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));
            assert_eq!(selected.rows[0][1], Value::Null);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_transaction_storage_failures.rs.
mod integration_sql_transaction_storage_failures {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
    };
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::midge::adapter::{
        document_write_failure_point_test_guard, set_document_write_failure_point,
        DocumentWriteFailurePoint,
    };
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_not_persist_row_when_row_family_failpoint_is_triggered() {
        // Arrange
        let _failpoint_guard = document_write_failure_point_test_guard();
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
}

// Formerly tests/integration_sql_transaction_visibility.rs.
mod integration_sql_transaction_visibility {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
    };
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::midge::adapter::{
        document_write_failure_point_test_guard, set_document_write_failure_point,
        DocumentWriteFailurePoint,
    };
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;
    #[test]
    fn should_hide_transaction_writes_from_other_sessions_before_commit() {
        // Arrange
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
        use_local_storage();
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
}

// Formerly tests/integration_sql_transactions.rs.
mod integration_sql_transactions {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
    };
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::midge::adapter::{
        document_write_failure_point_test_guard, set_document_write_failure_point,
        DocumentWriteFailurePoint,
    };
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
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
}

// Formerly tests/integration_sql_update.rs.
mod integration_sql_update {
    #![allow(unused_imports, dead_code)]
    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
    use cassie::embeddings::{
        openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord,
        VectorIndexType, DEFAULT_EMBEDDING_MODEL,
    };
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
    use cntryl_midge::{TransactionMode, WriteOptions};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_maintain_include_values_after_update_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("include_update_delete");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_include_update_delete (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_include_update_delete_email_idx ON sql_include_update_delete USING btree (email) INCLUDE (title)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_include_update_delete (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE sql_include_update_delete SET title = 'bravo' WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let updated = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_include_update_delete WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM sql_include_update_delete WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let deleted = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_include_update_delete WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(updated.rows, vec![vec![Value::String("bravo".to_string())]]);
        assert!(deleted.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_update_where_returning_rows() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_where_returning");
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
                "CREATE TABLE update_where_returning (title TEXT, status TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_where_returning (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_where_returning (title, status) VALUES ('beta', 'old')",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_where_returning SET status = 'done' WHERE title = 'alpha' RETURNING _id, title, status",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, status FROM update_where_returning ORDER BY title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(updated.command, "UPDATE 1");
        assert_eq!(updated.rows.len(), 1);
        assert!(matches!(&updated.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(updated.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(updated.rows[0][2], Value::String("done".to_string()));
        assert_eq!(selected.rows[0][1], Value::String("done".to_string()));
        assert_eq!(selected.rows[1][1], Value::String("old".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_update_returning_scalar_function() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_returning_function");
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
                "CREATE TABLE update_returning_function (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_returning_function (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_returning_function SET title = 'BETA' RETURNING lower(title) AS normalized",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(updated.columns[0].name, "normalized");
        assert_eq!(updated.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_row_id_when_update_rewrites_row_blob() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_preserve_id");
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
                "CREATE TABLE update_preserve_id (title TEXT, body TEXT)",
                vec![],
            )

.unwrap();
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO update_preserve_id (title, body) VALUES ('alpha', 'old') RETURNING _id",
                vec![],
            )
            .unwrap();
        let original_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_preserve_id SET body = 'new' WHERE title = 'alpha' RETURNING _id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(updated.rows[0][0], Value::String(original_id));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_update_validation_failure_without_mutating_row() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_validation_failure");
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
                    "CREATE TABLE update_validation_failure (title TEXT NOT NULL)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO update_validation_failure (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();

            // Act
            let updated = cassie.execute_sql(
                &session,
                "UPDATE update_validation_failure SET title = NULL RETURNING title",
                vec![],
            );
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM update_validation_failure",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(updated.is_err());
            assert!(updated.unwrap_err().to_string().contains("cannot be null"));
            assert_eq!(selected.rows[0][0], Value::String("alpha".to_string()));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_zero_rows_for_update_without_matches() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_no_match");
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
                "CREATE TABLE update_no_match (title TEXT, status TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO update_no_match (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE update_no_match SET status = 'done' WHERE title = 'missing' RETURNING title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(updated.command, "UPDATE 0");
        assert!(updated.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_update_with_duplicate_assignment_target() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_duplicate_assignment");
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
                    "CREATE TABLE update_duplicate_assignment (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let updated = cassie.execute_sql(
                &session,
                "UPDATE update_duplicate_assignment SET title = 'alpha', title = 'beta'",
                vec![],
            );

            // Assert
            assert!(updated.is_err());
            assert!(updated.unwrap_err().to_string().contains("duplicated"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_update_with_unknown_assignment_target() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_unknown_assignment");
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
                    "CREATE TABLE update_unknown_assignment (title TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let updated = cassie.execute_sql(
                &session,
                "UPDATE update_unknown_assignment SET missing = 'alpha'",
                vec![],
            );

            // Assert
            assert!(updated.is_err());
            assert!(updated.unwrap_err().to_string().contains("does not exist"));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/integration_sql_upsert.rs.
mod integration_sql_upsert {
    use cassie::app::Cassie;
    use cassie::types::Value;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn data_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cassie-upsert-{name}-{}", Uuid::new_v4()))
    }

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    #[test]
    fn should_update_conflicting_row_given_parameters_excluded_filter_and_returning() {
        // Arrange
        use_local_storage();
        let path = data_dir("update");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE upsert_docs (id INT PRIMARY KEY, tenant TEXT, title TEXT, note TEXT)", vec![]).unwrap();
            cassie.execute_sql(&session, "CREATE UNIQUE INDEX upsert_tenant_title ON upsert_docs (tenant, title)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO upsert_docs (id, tenant, title, note) VALUES (1, 'a', 'one', 'keep')", vec![]).unwrap();

            // Act
            let result = cassie.execute_sql(
                &session,
                "INSERT INTO upsert_docs (id, tenant, title) VALUES ($1, $2, $3) ON CONFLICT (tenant, title) DO UPDATE SET title = excluded.title WHERE upsert_docs.title = excluded.title RETURNING title, note",
                vec![Value::Int64(1), Value::String("a".into()), Value::String("one".into())],
            ).unwrap();

            // Assert
            assert_eq!(result.command, "INSERT 0 1");
            assert_eq!(result.rows, vec![vec![Value::String("one".into()), Value::String("keep".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_invalid_conflict_update_before_mutation() {
        // Arrange
        use_local_storage();
        let path = data_dir("binding");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE upsert_bind (id INT PRIMARY KEY, title TEXT)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'alpha')", vec![]).unwrap();

            // Act
            let unknown = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.missing", vec![]);
            let duplicate = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title, title = 'again'", vec![]);
            let non_unique = cassie.execute_sql(&session, "INSERT INTO upsert_bind VALUES (2, 'alpha') ON CONFLICT (title) DO UPDATE SET title = excluded.title", vec![]);
            let rows = cassie.execute_sql(&session, "SELECT title FROM upsert_bind", vec![]).unwrap();

            // Assert
            assert!(unknown.unwrap_err().to_string().contains("excluded.missing"));
            assert!(duplicate.unwrap_err().to_string().contains("duplicated"));
            assert!(non_unique.unwrap_err().to_string().contains("does not match"));
            assert_eq!(rows.rows, vec![vec![Value::String("alpha".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_persist_only_committed_upsert_given_rollback_then_commit() {
        // Arrange
        use_local_storage();
        let path = data_dir("transactions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE upsert_tx (id INT PRIMARY KEY, title TEXT)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'alpha')", vec![]).unwrap();

            // Act
            cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title", vec![]).unwrap();
            let during = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();
            cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
            let rolled_back = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();
            cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO upsert_tx VALUES (1, 'gamma') ON CONFLICT (id) DO UPDATE SET title = excluded.title", vec![]).unwrap();
            cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
            let committed = cassie.execute_sql(&session, "SELECT title FROM upsert_tx", vec![]).unwrap();

            // Assert
            assert_eq!(during.rows, vec![vec![Value::String("beta".into())]]);
            assert_eq!(rolled_back.rows, vec![vec![Value::String("alpha".into())]]);
            assert_eq!(committed.rows, vec![vec![Value::String("gamma".into())]]);
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_upsert_update_of_referenced_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign-key-restrict");
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
                    "CREATE TABLE upsert_restrict_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE upsert_restrict_children (parent_id INT REFERENCES upsert_restrict_parents(id), title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_restrict_parents VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_restrict_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            let result = cassie.execute_sql(
                &session,
                "INSERT INTO upsert_restrict_parents VALUES (1, 'alpha') ON CONFLICT (id) DO UPDATE SET id = 2",
                vec![],
            );
            let parents = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM upsert_restrict_parents",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(result
                .expect_err("referenced key update must be rejected")
                .to_string()
                .contains("still references"));
            assert_eq!(
                parents.rows,
                vec![vec![Value::String("alpha".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cascade_upsert_update_of_referenced_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign-key-cascade");
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
                    "CREATE TABLE upsert_cascade_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE upsert_cascade_children (parent_id INT, title TEXT, CONSTRAINT upsert_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES upsert_cascade_parents(id) ON UPDATE CASCADE)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_parents VALUES (1, 'alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO upsert_cascade_parents VALUES (1, 'alpha') ON CONFLICT (id) DO UPDATE SET id = 2",
                    vec![],
                )
                .unwrap();
            let children = cassie
                .execute_sql(
                    &session,
                    "SELECT parent_id FROM upsert_cascade_children",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(children.rows, vec![vec![Value::Int64(2)]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_transaction_conflict_resolution_of_referenced_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign-key-transaction-conflict");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let transaction_session = cassie.create_session("transaction", None);
            let concurrent_session = cassie.create_session("concurrent", None);
            cassie
                .execute_sql(
                    &transaction_session,
                    "CREATE TABLE upsert_tx_fk_parents (id INT PRIMARY KEY, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &transaction_session,
                    "CREATE TABLE upsert_tx_fk_children (parent_id INT REFERENCES upsert_tx_fk_parents(id), title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&transaction_session, "BEGIN", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &transaction_session,
                    "INSERT INTO upsert_tx_fk_parents VALUES (1, 'staged') ON CONFLICT (id) DO UPDATE SET id = 2",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &concurrent_session,
                    "INSERT INTO upsert_tx_fk_parents VALUES (1, 'committed')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &concurrent_session,
                    "INSERT INTO upsert_tx_fk_children VALUES (1, 'child')",
                    vec![],
                )
                .unwrap();

            // Act
            let commit = cassie.execute_sql(&transaction_session, "COMMIT", vec![]);
            let parents = cassie
                .execute_sql(
                    &concurrent_session,
                    "SELECT title FROM upsert_tx_fk_parents",
                    vec![],
                )
                .unwrap();
            let children = cassie
                .execute_sql(
                    &concurrent_session,
                    "SELECT parent_id FROM upsert_tx_fk_children",
                    vec![],
                )
                .unwrap();

            // Assert
            assert!(commit
                .expect_err("commit-time upsert must preserve referential integrity")
                .to_string()
                .contains("still references"));
            assert_eq!(
                parents.rows,
                vec![vec![Value::String("committed".to_string())]]
            );
            assert_eq!(children.rows, vec![vec![Value::Int64(1)]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/integration_sql_upsert_concurrency.rs.
mod integration_sql_upsert_concurrency {
    use std::sync::{Arc, Barrier};

    use cassie::app::Cassie;

    use super::support_sql as support;

    #[test]
    fn should_resolve_racing_do_nothing_without_unique_error() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("upsert-racing-do-nothing");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE racing_upserts (email TEXT UNIQUE, value INT)",
                vec![],
            )
            .expect("create table");
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for value in [1, 2] {
            let cassie = cassie.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
            let session = cassie.create_session("tester", None);
            barrier.wait();
            cassie.execute_sql(
                &session,
                &format!(
                    "INSERT INTO racing_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO NOTHING"
                ),
                vec![],
            )
        }));
        }

        // Act
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .collect::<Vec<_>>();

        // Assert
        assert!(results.iter().all(Result::is_ok));
        let rows = cassie
            .execute_sql(&setup, "SELECT email FROM racing_upserts", vec![])
            .expect("read winner");
        assert_eq!(rows.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_resolve_one_racing_do_update_against_committed_winner() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("upsert-racing-do-update");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE racing_update_upserts (email TEXT UNIQUE, value INT)",
                vec![],
            )
            .expect("create table");
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for value in [1, 2] {
            let cassie = cassie.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
            let session = cassie.create_session("tester", None);
            barrier.wait();
            cassie.execute_sql(
                &session,
                &format!(
                    "INSERT INTO racing_update_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO UPDATE SET value = excluded.value RETURNING value"
                ),
                vec![],
            )
        }));
        }

        // Act
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker"))
            .collect::<Vec<_>>();

        // Assert
        assert!(results.iter().all(Result::is_ok));
        assert!(results
            .iter()
            .all(|result| result.as_ref().expect("upsert").rows.len() == 1));
        let rows = cassie
            .execute_sql(&setup, "SELECT value FROM racing_update_upserts", vec![])
            .expect("read resolved row");
        assert_eq!(rows.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_resolve_transactional_do_nothing_when_competing_commit_wins() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("upsert-transaction-do-nothing");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE transaction_upserts (email TEXT UNIQUE, value INT)",
                vec![],
            )
            .expect("create table");
        let first = cassie.create_session("tester", None);
        let second = cassie.create_session("tester", None);
        for session in [&first, &second] {
            cassie
                .execute_sql(session, "BEGIN", vec![])
                .expect("begin transaction");
        }
        for (session, value) in [(&first, 1), (&second, 2)] {
            cassie
            .execute_sql(
                session,
                &format!(
                    "INSERT INTO transaction_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO NOTHING"
                ),
                vec![],
            )
            .expect("stage upsert");
        }

        // Act
        cassie
            .execute_sql(&first, "COMMIT", vec![])
            .expect("commit winner");
        let losing_commit = cassie.execute_sql(&second, "COMMIT", vec![]);

        // Assert
        losing_commit.expect("resolve losing transaction");
        let rows = cassie
            .execute_sql(&setup, "SELECT email FROM transaction_upserts", vec![])
            .expect("read winner");
        assert_eq!(rows.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_resolve_transactional_do_update_against_committed_winner() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("upsert-transaction-do-update");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE transaction_update_upserts (email TEXT UNIQUE, value INT)",
                vec![],
            )
            .expect("create table");
        let first = cassie.create_session("tester", None);
        let second = cassie.create_session("tester", None);
        for session in [&first, &second] {
            cassie
                .execute_sql(session, "BEGIN", vec![])
                .expect("begin transaction");
        }
        for (session, value) in [(&first, 1), (&second, 2)] {
            cassie
            .execute_sql(
                session,
                &format!(
                    "INSERT INTO transaction_update_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO UPDATE SET value = excluded.value"
                ),
                vec![],
            )
            .expect("stage upsert");
        }

        // Act
        cassie
            .execute_sql(&first, "COMMIT", vec![])
            .expect("commit winner");
        cassie
            .execute_sql(&second, "COMMIT", vec![])
            .expect("resolve update transaction");

        // Assert
        let rows = cassie
            .execute_sql(
                &setup,
                "SELECT value FROM transaction_update_upserts",
                vec![],
            )
            .expect("read resolved row");
        assert_eq!(rows.rows, vec![vec![cassie::types::Value::Int64(2)]]);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_restore_transactional_conflict_intent_with_savepoint_rollback() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("upsert-transaction-savepoint");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE savepoint_upserts (email TEXT UNIQUE, value INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(&session, "SAVEPOINT before_upsert", vec![])
            .expect("create savepoint");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO savepoint_upserts (email, value) VALUES ('rolled-back@example.com', 1) ON CONFLICT (email) DO NOTHING",
            vec![],
        )
        .expect("stage upsert");

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT before_upsert", vec![])
            .expect("rollback intent");
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("commit transaction");

        // Assert
        let rows = cassie
            .execute_sql(&session, "SELECT email FROM savepoint_upserts", vec![])
            .expect("read rows");
        assert!(rows.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/migration_ddl_sequences.rs.
mod migration_ddl_sequences {
    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;
    use std::path::Path;

    use super::support_data_dir as data_dir;
    use super::support_local_storage as local_storage;
    use data_dir::data_dir;
    use local_storage::use_local_storage;

    struct SequenceRestartState {
        after_restart: Vec<Vec<Value>>,
        columns: Vec<Vec<Value>>,
        attrdefs: Vec<Vec<Value>>,
        sequences: Vec<Vec<Value>>,
        sequence_class: Vec<Vec<Value>>,
        dropped_sequence: Vec<Vec<Value>>,
        unsupported_error: String,
    }

    fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }

    fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie.execute_sql(session, sql, vec![]).unwrap().rows
    }

    fn create_sequence_default_schema(cassie: &Cassie, session: &CassieSession) {
        execute_statement(cassie, session, "CREATE SEQUENCE order_ids");
        execute_statement(
            cassie,
            session,
            "CREATE TABLE migration_orders (
            seq_id INT DEFAULT nextval('order_ids'::regclass),
            label TEXT
        )",
        );
        execute_statement(
            cassie,
            session,
            "CREATE TABLE migration_source (label TEXT)",
        );
    }

    fn apply_sequence_default_mutations(cassie: &Cassie, session: &CassieSession) {
        for sql in [
            "INSERT INTO migration_orders (label) VALUES ('alpha')",
            "INSERT INTO migration_source (label) VALUES ('beta')",
            "INSERT INTO migration_orders (label) SELECT label FROM migration_source",
            "ALTER TABLE migration_orders ALTER COLUMN label SET DEFAULT 'pending'",
            "INSERT INTO migration_orders (seq_id) VALUES (10)",
            "ALTER TABLE migration_orders ALTER COLUMN label SET NOT NULL",
            "ALTER TABLE migration_orders ALTER COLUMN label DROP NOT NULL",
            "ALTER TABLE migration_orders ALTER COLUMN label DROP DEFAULT",
        ] {
            execute_statement(cassie, session, sql);
        }
    }

    fn migration_orders_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
        query_rows(
            cassie,
            session,
            "SELECT seq_id, label FROM migration_orders ORDER BY seq_id",
        )
    }

    fn restart_and_collect_sequence_state(path: &Path) -> SequenceRestartState {
        let restarted = Cassie::new_with_data_dir(path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        execute_statement(
            &restarted,
            &session,
            "INSERT INTO migration_orders (label) VALUES ('gamma')",
        );
        execute_statement(&restarted, &session, "CREATE SEQUENCE temp_ids");
        execute_statement(&restarted, &session, "DROP SEQUENCE temp_ids");

        let unsupported = restarted
            .execute_sql(
                &session,
                "CREATE SEQUENCE unsupported_ids START WITH 5",
                vec![],
            )
            .expect_err("unsupported sequence option should fail")
            .to_string();

        SequenceRestartState {
        after_restart: migration_orders_rows(&restarted, &session),
        columns: query_rows(
            &restarted,
            &session,
            "SELECT column_name, column_default, is_nullable FROM information_schema.columns WHERE table_name = 'migration_orders' ORDER BY ordinal_position",
        ),
        attrdefs: query_rows(
            &restarted,
            &session,
            "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'migration_orders' ORDER BY adnum",
        ),
        sequences: query_rows(
            &restarted,
            &session,
            "SELECT sequence_name, data_type, start_value, increment FROM information_schema.sequences WHERE sequence_name = 'order_ids'",
        ),
        sequence_class: query_rows(
            &restarted,
            &session,
            "SELECT relname, relkind FROM pg_catalog.pg_class WHERE relname = 'order_ids'",
        ),
        dropped_sequence: query_rows(
            &restarted,
            &session,
            "SELECT sequence_name FROM information_schema.sequences WHERE sequence_name = 'temp_ids'",
        ),
        unsupported_error: unsupported,
    }
    }

    fn assert_sequence_default_state(before_restart: &[Vec<Value>], state: &SequenceRestartState) {
        assert_eq!(before_restart, expected_orders_before_restart().as_slice());
        assert_eq!(state.after_restart, expected_orders_after_restart());
        assert_eq!(state.columns, expected_column_rows());
        assert_eq!(state.attrdefs, expected_attrdef_rows());
        assert_eq!(state.sequences, expected_sequence_rows());
        assert_eq!(state.sequence_class, expected_sequence_class_rows());
        assert!(state.dropped_sequence.is_empty());
        assert!(state
            .unsupported_error
            .contains("unsupported CREATE SEQUENCE option"));
    }

    fn expected_orders_before_restart() -> Vec<Vec<Value>> {
        vec![
            vec![Value::Int64(1), Value::String("alpha".to_string())],
            vec![Value::Int64(2), Value::String("beta".to_string())],
            vec![Value::Int64(10), Value::String("pending".to_string())],
        ]
    }

    fn expected_orders_after_restart() -> Vec<Vec<Value>> {
        vec![
            vec![Value::Int64(1), Value::String("alpha".to_string())],
            vec![Value::Int64(2), Value::String("beta".to_string())],
            vec![Value::Int64(3), Value::String("gamma".to_string())],
            vec![Value::Int64(10), Value::String("pending".to_string())],
        ]
    }

    fn expected_column_rows() -> Vec<Vec<Value>> {
        vec![
            vec![
                Value::String("seq_id".to_string()),
                Value::String("nextval('order_ids'::regclass)".to_string()),
                Value::String("YES".to_string()),
            ],
            vec![
                Value::String("label".to_string()),
                Value::Null,
                Value::String("YES".to_string()),
            ],
        ]
    }

    fn expected_attrdef_rows() -> Vec<Vec<Value>> {
        vec![vec![
            Value::String("migration_orders".to_string()),
            Value::Int64(1),
            Value::String("nextval('order_ids'::regclass)".to_string()),
        ]]
    }

    fn expected_sequence_rows() -> Vec<Vec<Value>> {
        vec![vec![
            Value::String("order_ids".to_string()),
            Value::String("integer".to_string()),
            Value::String("1".to_string()),
            Value::String("1".to_string()),
        ]]
    }

    fn expected_sequence_class_rows() -> Vec<Vec<Value>> {
        vec![vec![
            Value::String("order_ids".to_string()),
            Value::String("S".to_string()),
        ]]
    }

    #[test]
    fn should_apply_sequence_defaults_metadata_through_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("sequence-defaults");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            create_sequence_default_schema(&cassie, &session);

            // Act
            apply_sequence_default_mutations(&cassie, &session);
            let before_restart = migration_orders_rows(&cassie, &session);
            drop(cassie);
            let state = restart_and_collect_sequence_state(&path);

            // Assert
            assert_sequence_default_state(&before_restart, &state);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_desugar_serial_columns_to_sequence_backed_integer_defaults() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial");
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
                "CREATE TABLE serial_orders (
                    serial_id SERIAL,
                    ledger_id BIGSERIAL,
                    label TEXT
                )",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO serial_orders (label) VALUES ('one')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO serial_orders (label) VALUES ('two')",
                vec![],
            )
            .unwrap();

        let rows = cassie
            .execute_sql(
                &session,
                "SELECT serial_id, ledger_id, label FROM serial_orders ORDER BY serial_id",
                vec![],
            )
            .unwrap();
        let columns = cassie
            .execute_sql(
                &session,
                "SELECT column_name, data_type, column_default, is_nullable FROM information_schema.columns WHERE table_name = 'serial_orders' ORDER BY ordinal_position",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            rows.rows,
            vec![
                vec![
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::String("one".to_string()),
                ],
                vec![
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("two".to_string()),
                ],
            ]
        );
        assert_eq!(
            columns.rows,
            vec![
                vec![
                    Value::String("serial_id".to_string()),
                    Value::String("int".to_string()),
                    Value::String("nextval('serial_orders_serial_id_seq'::regclass)".to_string()),
                    Value::String("NO".to_string()),
                ],
                vec![
                    Value::String("ledger_id".to_string()),
                    Value::String("bigint".to_string()),
                    Value::String("nextval('serial_orders_ledger_id_seq'::regclass)".to_string()),
                    Value::String("NO".to_string()),
                ],
                vec![
                    Value::String("label".to_string()),
                    Value::String("text".to_string()),
                    Value::Null,
                    Value::String("YES".to_string()),
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/transaction_commit_boundary.rs.
mod transaction_commit_boundary {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::executor::set_materialized_projection_maintenance_failure_point;
    use cassie::midge::adapter::set_rollup_maintenance_failure_point;
    use cassie::types::Value;

    static TRANSACTION_MAINTENANCE_FAILPOINT_GUARD: std::sync::Mutex<()> =
        std::sync::Mutex::new(());

    use support::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    #[test]
    fn should_not_retry_a_durable_commit_after_materialized_refresh_failure() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = TRANSACTION_MAINTENANCE_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("transaction_commit_materialized_boundary");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE transaction_materialized_source (tenant TEXT, amount INT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO transaction_materialized_source (tenant, amount) VALUES ('acme', 10)",
                    vec![],
                )
                .expect("seed source");
            cassie
                .execute_sql(
                    &session,
                    "CREATE MATERIALIZED PROJECTION transaction_materialized WITH (analytical = true) AS SELECT tenant, amount FROM transaction_materialized_source",
                    vec![],
                )
                .expect("create projection");
            cassie
                .execute_sql(&session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO transaction_materialized_source (tenant, amount) VALUES ('acme', 20)",
                    vec![],
                )
                .expect("stage write");
            set_materialized_projection_maintenance_failure_point(true);

            // Act
            let commit = cassie
                .execute_sql(&session, "COMMIT", vec![])
                .expect("base commit remains successful");
            let retry = cassie.execute_sql(&session, "COMMIT", vec![]);
            let rollback = cassie
                .execute_sql(&session, "ROLLBACK", vec![])
                .expect("rollback after a durable commit remains harmless");
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM transaction_materialized_source ORDER BY amount",
                    vec![],
                )
                .expect("read committed source");
            let debt = cassie
                .execute_sql(
                    &session,
                    "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_materialized_source'",
                    vec![],
                )
                .expect("read maintenance debt");

            // Assert
            assert_eq!(commit.command, "COMMIT");
            assert!(retry.is_err(), "a durable COMMIT must not be retryable");
            assert_eq!(rollback.command, "ROLLBACK");
            assert_eq!(session.transaction_status(), "idle");
            assert_eq!(
                rows.rows,
                vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
            );
            assert_eq!(
                debt.rows,
                vec![vec![Value::String("materialized_projection".to_string())]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_replay_rollup_debt_after_durable_transaction_commit() {
        // Arrange
        use_local_storage();
        let _failpoint_guard = TRANSACTION_MAINTENANCE_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("transaction_commit_rollup_boundary");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE transaction_rollup_source (tenant TEXT, event_at TEXT, amount INT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO transaction_rollup_source (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:05:00Z', 10)",
                    vec![],
                )
                .expect("seed source");
            cassie
                .execute_sql(
                    &session,
                    "CREATE ROLLUP transaction_rollup ON transaction_rollup_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                    vec![],
                )
                .expect("create rollup");
            cassie
                .execute_sql(&session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO transaction_rollup_source (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:25:00Z', 20)",
                    vec![],
                )
                .expect("stage write");
            set_rollup_maintenance_failure_point(true);

            // Act
            let commit = cassie
                .execute_sql(&session, "COMMIT", vec![])
                .expect("base commit remains successful");
            let retry = cassie.execute_sql(&session, "COMMIT", vec![]);
            let source_rows = cassie
                .execute_sql(
                    &session,
                    "SELECT amount FROM transaction_rollup_source ORDER BY amount",
                    vec![],
                )
                .expect("read committed source");
            let debt = cassie
                .execute_sql(
                    &session,
                    "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_rollup_source'",
                    vec![],
                )
                .expect("read maintenance debt");
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
            restarted.startup().expect("restart startup");
            let restarted_session = restarted.create_session("tester", None);
            let rollup_rows = restarted
                .execute_sql(
                    &restarted_session,
                    "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM transaction_rollup_source GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant",
                    vec![],
                )
                .expect("read recovered rollup");
            let remaining_debt = restarted
                .execute_sql(
                    &restarted_session,
                    "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_rollup_source'",
                    vec![],
                )
                .expect("read recovered maintenance debt");

            // Assert
            assert_eq!(commit.command, "COMMIT");
            assert!(retry.is_err(), "a durable COMMIT must not be retryable");
            assert_eq!(session.transaction_status(), "idle");
            assert_eq!(
                source_rows.rows,
                vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
            );
            assert_eq!(
                debt.rows,
                vec![vec![Value::String("rollup".to_string())]]
            );
            assert_eq!(
                rollup_rows.rows,
                vec![vec![
                    Value::String("2026-01-01T00:00:00Z".to_string()),
                    Value::String("acme".to_string()),
                    Value::Int64(2),
                    Value::Int64(30),
                ]]
            );
            assert!(remaining_debt.rows.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
// Formerly tests/transaction_semantics.rs.
mod transaction_semantics {
    use cassie::app::{Cassie, CassieError};
    use cassie::sql::ast::{CopyFormat, CopyStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn with_source_table<T>(
        label: &str,
        test: impl FnOnce(&Cassie, &cassie::app::CassieSession) -> T,
    ) -> T {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_semantics_source (id INT PRIMARY KEY, title TEXT, tenant TEXT, event_at TIMESTAMP, amount INT)",
            vec![],
        )
        .expect("create source table");
        let result = test(&cassie, &session);
        let _ = std::fs::remove_dir_all(path);
        result
    }

    fn assert_unsupported(error: &CassieError) {
        assert!(matches!(error, CassieError::Unsupported(_)));
    }

    fn reject_active_transaction_command(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        sql: &str,
    ) {
        cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
        let error = cassie
            .execute_sql(session, sql, vec![])
            .expect_err("command should be rejected in an active transaction");
        assert_unsupported(&error);
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback rejected command");
    }

    #[test]
    fn should_reject_non_read_committed_begin() {
        // Arrange
        with_source_table("transaction_semantics_isolation", |cassie, session| {
            for sql in [
                "BEGIN ISOLATION LEVEL SERIALIZABLE",
                "BEGIN ISOLATION LEVEL REPEATABLE READ",
            ] {
                // Act
                let error = cassie
                    .execute_sql(session, sql, vec![])
                    .expect_err("unsupported isolation should fail before BEGIN");

                // Assert
                assert_unsupported(&error);
                assert_eq!(session.transaction_status(), "idle");
            }
        });
    }

    #[test]
    fn should_allow_explicit_read_committed_begin() {
        // Arrange
        with_source_table("transaction_semantics_read_committed", |cassie, session| {
            // Act
            let result = cassie
                .execute_sql(session, "BEGIN ISOLATION LEVEL READ COMMITTED", vec![])
                .expect("read committed should be supported");

            // Assert
            assert_eq!(result.command, "BEGIN");
            assert_eq!(session.transaction_status(), "in_transaction");
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback read committed transaction");
        });
    }

    #[test]
    fn should_reject_set_transaction_in_active_transaction() {
        // Arrange
        with_source_table("transaction_semantics_set", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");

            // Act
            let error = cassie
                .execute_sql(
                    session,
                    "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
                    vec![],
                )
                .expect_err("SET TRANSACTION should be rejected");

            // Assert
            assert_unsupported(&error);
            assert_eq!(session.transaction_status(), "failed");
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback SET TRANSACTION rejection");
        });
    }

    #[test]
    fn should_reject_ddl_in_active_transaction() {
        // Arrange
        with_source_table("transaction_semantics_ddl", |cassie, session| {
            for sql in [
            "CREATE TABLE transaction_semantics_new_table (value TEXT)",
            "ALTER TABLE transaction_semantics_source ADD COLUMN rejected TEXT",
            "CREATE SCHEMA transaction_semantics_schema",
            "CREATE INDEX transaction_semantics_new_index ON transaction_semantics_source (title)",
            "CREATE VIEW transaction_semantics_new_view AS SELECT title FROM transaction_semantics_source",
            "CREATE SEQUENCE transaction_semantics_new_sequence",
            "CREATE ROLLUP transaction_semantics_new_rollup ON transaction_semantics_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
            "CREATE MATERIALIZED PROJECTION transaction_semantics_new_projection AS SELECT title FROM transaction_semantics_source",
        ] {
            reject_active_transaction_command(cassie, session, sql);
        }

            // Act
            let source = cassie
                .catalog
                .get_schema("transaction_semantics_source")
                .expect("source schema");

            // Assert
            assert!(!cassie
                .catalog
                .relation_exists("transaction_semantics_new_table"));
            assert!(!cassie
                .catalog
                .namespace_exists("transaction_semantics_schema"));
            assert!(!source.fields.iter().any(|field| field.name == "rejected"));
            assert!(cassie
                .catalog
                .get_index(
                    "transaction_semantics_source",
                    "transaction_semantics_new_index"
                )
                .is_none());
            assert!(cassie
                .catalog
                .get_view("transaction_semantics_new_view")
                .is_none());
            assert!(!cassie
                .catalog
                .sequence_exists("transaction_semantics_new_sequence"));
            assert!(cassie
                .catalog
                .get_rollup("transaction_semantics_new_rollup")
                .is_none());
            assert!(cassie
                .catalog
                .get_materialized_projection("transaction_semantics_new_projection")
                .is_none());
        });
    }

    #[test]
    fn should_preserve_staged_data_after_ddl_rejection() {
        // Arrange
        with_source_table("transaction_semantics_ddl_state", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_semantics_source (id, title) VALUES (1, 'staged')",
                    vec![],
                )
                .expect("stage source row");

            // Act
            let error = cassie
                .execute_sql(
                    session,
                    "CREATE TABLE transaction_semantics_rejected (value TEXT)",
                    vec![],
                )
                .expect_err("DDL should fail after staged DML");

            // Assert
            assert_unsupported(&error);
            assert_eq!(session.transaction_status(), "failed");
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback DDL rejection");
            let rows = cassie
                .execute_sql(
                    session,
                    "SELECT title FROM transaction_semantics_source",
                    vec![],
                )
                .expect("read source after rollback");
            assert!(rows.rows.is_empty());
            assert!(!cassie
                .catalog
                .relation_exists("transaction_semantics_rejected"));
        });
    }

    #[test]
    fn should_stage_copy_in_active_transaction() {
        // Arrange
        with_source_table("transaction_semantics_copy", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            let statement = CopyStatement {
                table: "transaction_semantics_source".to_string(),
                columns: vec!["id".to_string(), "title".to_string()],
                format: CopyFormat::Csv,
                header: false,
            };

            // Act
            cassie
                .copy_from_csv_stdin(session, &statement, b"1,copied\n")
                .expect("stage COPY rows");

            // Assert
            assert_eq!(session.transaction_status(), "in_transaction");
            let staged = cassie
                .execute_sql(
                    session,
                    "SELECT title FROM transaction_semantics_source",
                    vec![],
                )
                .expect("read staged COPY row");
            assert_eq!(staged.rows, vec![vec![Value::String("copied".into())]]);
            cassie
                .execute_sql(session, "COMMIT", vec![])
                .expect("commit COPY");
            let rows = cassie
                .execute_sql(
                    session,
                    "SELECT title FROM transaction_semantics_source",
                    vec![],
                )
                .expect("read source after COPY rollback");
            assert_eq!(rows.rows, vec![vec![Value::String("copied".into())]]);
        });
    }
}

// Formerly tests/transaction_staging.rs.
mod transaction_staging {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn with_two_collections<T>(
        label: &str,
        test: impl FnOnce(&Cassie, &cassie::app::CassieSession) -> T,
    ) -> T {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_stage_a (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create first collection");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_stage_b (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create second collection");
        let result = test(&cassie, &session);
        let _ = std::fs::remove_dir_all(path);
        result
    }

    #[test]
    fn should_stage_writes_across_collections() {
        // Arrange
        with_two_collections("transaction_stage_write", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .expect("stage first collection");

            // Act
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                    vec![],
                )
                .expect("stage second collection");
            cassie
                .execute_sql(session, "COMMIT", vec![])
                .expect("commit");

            // Assert
            assert_eq!(session.transaction_status(), "idle");
        });
    }

    #[test]
    fn should_stage_delete_across_collections() {
        // Arrange
        with_two_collections("transaction_stage_delete", |cassie, session| {
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                    vec![],
                )
                .expect("seed second collection");
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .expect("stage first collection");

            // Act
            cassie
                .execute_sql(
                    session,
                    "DELETE FROM transaction_stage_b WHERE title = 'beta'",
                    vec![],
                )
                .expect("stage second collection delete");
            cassie
                .execute_sql(session, "COMMIT", vec![])
                .expect("commit");

            // Assert
            let rows = cassie
                .execute_sql(session, "SELECT id FROM transaction_stage_b", vec![])
                .expect("read deleted collection");
            assert!(rows.rows.is_empty());
        });
    }

    #[test]
    fn should_discard_multi_collection_staged_rows_after_rollback() {
        // Arrange
        with_two_collections("transaction_stage_rollback", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .expect("stage first collection");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                    vec![],
                )
                .expect("stage second collection");

            // Act
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback transaction");
            let first_rows = cassie
                .execute_sql(session, "SELECT id FROM transaction_stage_a", vec![])
                .expect("read first collection");

            // Assert
            assert!(first_rows.rows.is_empty());
            assert_eq!(session.transaction_status(), "idle");
        });
    }

    #[test]
    fn should_recover_after_multi_collection_rollback() {
        // Arrange
        with_two_collections("transaction_stage_recovery", |cassie, session| {
            cassie.execute_sql(session, "BEGIN", vec![]).expect("begin");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_a (id, title) VALUES (1, 'alpha')",
                    vec![],
                )
                .expect("stage first collection");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                    vec![],
                )
                .expect("stage second collection");
            cassie
                .execute_sql(session, "ROLLBACK", vec![])
                .expect("rollback transaction");

            // Act
            cassie
                .execute_sql(session, "BEGIN", vec![])
                .expect("begin retry");
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO transaction_stage_b (id, title) VALUES (1, 'beta')",
                    vec![],
                )
                .expect("stage collection after rollback");
            cassie
                .execute_sql(session, "COMMIT", vec![])
                .expect("commit retry");
            let rows = cassie
                .execute_sql(
                    session,
                    "SELECT title FROM transaction_stage_b WHERE id = 1",
                    vec![],
                )
                .expect("read committed retry");

            // Assert
            assert_eq!(rows.rows, vec![vec![Value::String("beta".to_string())]]);
        });
    }

    #[test]
    fn should_commit_cross_collection_delete_cascade() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction_stage_cascade");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_cascade_parent (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create parent");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_cascade_child (id INT PRIMARY KEY, parent_id INT, CONSTRAINT transaction_cascade_child_parent FOREIGN KEY (parent_id) REFERENCES transaction_cascade_parent(id) ON DELETE CASCADE)",
            vec![],
        )
        .expect("create child");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_cascade_parent (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .expect("seed parent");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_cascade_child (id, parent_id) VALUES (1, 1)",
                vec![],
            )
            .expect("seed child");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin");

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM transaction_cascade_parent WHERE title = 'alpha'",
                vec![],
            )
            .expect("stage cross-collection cascade");
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("commit cascade");

        // Assert
        let parent = cassie
            .execute_sql(
                &session,
                "SELECT id FROM transaction_cascade_parent",
                vec![],
            )
            .expect("read parent after rollback");
        let child = cassie
            .execute_sql(&session, "SELECT id FROM transaction_cascade_child", vec![])
            .expect("read child after rollback");
        assert!(parent.rows.is_empty());
        assert!(child.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_commit_cross_collection_update_cascade() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction_stage_update_cascade");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_update_parent (id INT PRIMARY KEY, code TEXT UNIQUE, title TEXT)",
            vec![],
        )
        .expect("create parent");
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE transaction_update_child (parent_code TEXT, title TEXT, CONSTRAINT transaction_update_child_parent FOREIGN KEY (parent_code) REFERENCES transaction_update_parent(code) ON UPDATE CASCADE)",
            vec![],
        )
        .expect("create child");
        cassie
        .execute_sql(
            &session,
            "INSERT INTO transaction_update_parent (id, code, title) VALUES (1, 'one', 'alpha')",
            vec![],
        )
        .expect("seed parent");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_update_child (parent_code, title) VALUES ('one', 'child')",
                vec![],
            )
            .expect("seed child");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin");

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE transaction_update_parent SET code = 'two' WHERE title = 'alpha'",
                vec![],
            )
            .expect("stage cross-collection update cascade");
        cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("commit update cascade");

        // Assert
        let parent = cassie
            .execute_sql(
                &session,
                "SELECT code FROM transaction_update_parent",
                vec![],
            )
            .expect("read parent after rollback");
        let child = cassie
            .execute_sql(
                &session,
                "SELECT parent_code FROM transaction_update_child",
                vec![],
            )
            .expect("read child after rollback");
        assert_eq!(parent.rows, vec![vec![Value::String("two".to_string())]]);
        assert_eq!(child.rows, vec![vec![Value::String("two".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/unique_reservations.rs.
mod unique_reservations {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;

    #[test]
    fn should_release_unique_reservation_when_document_is_deleted() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_delete");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_reservation_delete (email TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
                vec![],
            )
            .expect("insert original row");

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM unique_reservation_delete WHERE email = 'reuse@example.com'",
                vec![],
            )
            .expect("delete original row");
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
                vec![],
            )
            .expect("reuse unique value");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT email FROM unique_reservation_delete",
                vec![],
            )
            .expect("select rows");

        // Assert
        assert_eq!(inserted.command, "INSERT 0 1");
        assert_eq!(
            rows.rows,
            vec![vec![Value::String("reuse@example.com".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
