// Consolidated integration suite: sql_mutations.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/data_dir.rs"]
mod support_data_dir;
#[path = "support/local_storage.rs"]
mod support_local_storage;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

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

mod copy_array_text_input {
    use cassie::app::Cassie;
    use cassie::sql::ast::{CopyFormat, CopyStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_copy_postgres_array_text_into_array_columns() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy-postgres-array-text");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE copy_array_text_rows (id TEXT, tags TEXT[])",
                vec![],
            )
            .expect("create array table");
        let statement = CopyStatement {
            table: "copy_array_text_rows".to_string(),
            columns: vec!["id".to_string(), "tags".to_string()],
            format: CopyFormat::Csv,
            header: false,
        };

        // Act
        let copied =
            cassie.copy_from_csv_stdin(&session, &statement, b"empty,{}\nitems,\"{a,b}\"\n");

        // Assert
        assert_eq!(copied.expect("PostgreSQL CSV arrays should load"), 2);
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id, tags FROM copy_array_text_rows ORDER BY id",
                vec![],
            )
            .expect("read copied arrays")
            .rows;
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::String("empty".into()),
                    Value::Json(serde_json::json!([]))
                ],
                vec![
                    Value::String("items".into()),
                    Value::Json(serde_json::json!(["a", "b"])),
                ],
            ]
        );
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}

mod copy_csv_null_fields {
    use cassie::app::Cassie;
    use cassie::sql::ast::{CopyFormat, CopyStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    #[test]
    fn should_copy_unquoted_empty_csv_fields_as_null() {
        // Arrange
        use_local_storage();
        let path = data_dir("copy-csv-empty-null");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE copy_csv_null_rows (id INT, flag BOOLEAN, note TEXT)",
                vec![],
            )
            .expect("create table");
        let statement = CopyStatement {
            table: "copy_csv_null_rows".to_string(),
            columns: vec!["id".to_string(), "flag".to_string(), "note".to_string()],
            format: CopyFormat::Csv,
            header: false,
        };

        // Act
        let copied =
            cassie.copy_from_csv_stdin(&session, &statement, b"20,t,\"\"\n21,,\n22,f,kept\n");

        // Assert
        assert_eq!(copied.expect("PostgreSQL CSV NULLs should load"), 3);
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id, flag IS NULL, note IS NULL, note FROM copy_csv_null_rows ORDER BY id",
                vec![],
            )
            .expect("read copied rows")
            .rows;
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::Int64(20),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::String(String::new()),
                ],
                vec![
                    Value::Int64(21),
                    Value::Bool(true),
                    Value::Bool(true),
                    Value::Null,
                ],
                vec![
                    Value::Int64(22),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::String("kept".into()),
                ],
            ]
        );
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}

mod alter_add_column_constraints {
    use cassie::app::Cassie;
    use cassie::types::Value;

    use super::support_sql as support;

    #[test]
    fn should_enforce_inline_constraints_added_to_existing_table() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("alter-add-column-inline-constraints");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE alter_add_column_parent (id INT PRIMARY KEY)",
            "INSERT INTO alter_add_column_parent (id) VALUES (1)",
            "CREATE TABLE alter_add_column_constraints (id INT PRIMARY KEY)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN required_value INT NOT NULL DEFAULT 5",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN unique_value INT UNIQUE",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN checked_value INT CHECK (checked_value > 0)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN referenced_value INT REFERENCES alter_add_column_parent(id)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN serial_value SERIAL",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO alter_add_column_constraints (id, unique_value, checked_value, referenced_value) VALUES (1, 7, 1, 1)",
                vec![],
            )
            .expect("insert row omitting the defaulted column");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT required_value, serial_value FROM alter_add_column_constraints WHERE id = 1",
                vec![],
            )
            .expect("read defaulted and generated values")
            .rows;
        let null_not_null = cassie.execute_sql(
            &session,
            "INSERT INTO alter_add_column_constraints (id, required_value) VALUES (2, NULL)",
            vec![],
        );
        let duplicate_unique = cassie.execute_sql(
            &session,
            "INSERT INTO alter_add_column_constraints (id, unique_value) VALUES (3, 7)",
            vec![],
        );
        let check_violation = cassie.execute_sql(
            &session,
            "INSERT INTO alter_add_column_constraints (id, checked_value) VALUES (4, -1)",
            vec![],
        );
        let foreign_key_violation = cassie.execute_sql(
            &session,
            "INSERT INTO alter_add_column_constraints (id, referenced_value) VALUES (5, 999)",
            vec![],
        );
        let serial_sequence_exists = cassie
            .catalog
            .get_sequence(&cassie::catalog::serial_sequence_name(
                "alter_add_column_constraints",
                "serial_value",
            ))
            .is_some();
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);

        // Assert
        assert_eq!(
            (
                inserted.command.as_str(),
                rows,
                null_not_null.is_err(),
                duplicate_unique.is_err(),
                check_violation.is_err(),
                foreign_key_violation.is_err(),
                serial_sequence_exists,
            ),
            (
                "INSERT 0 1",
                vec![vec![Value::Int64(5), Value::Int64(1)]],
                true,
                true,
                true,
                true,
                true,
            ),
            "ALTER TABLE ADD COLUMN must retain defaults, constraints, references, and serial behavior"
        );
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
        crate::support_temp_dirs::sweep_stale_once();
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
    use cassie::sql::ast::{CopyFormat, CopyStatement};

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
                "CREATE TABLE fk_race_parents (pid INT PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE fk_race_children (parent_id INT REFERENCES fk_race_parents(pid))",
                vec![],
            )
            .expect("create child table");

        for attempt in 0..32 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO fk_race_parents (pid) VALUES (1)",
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
                    "DELETE FROM fk_race_parents WHERE pid = 1",
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

    fn copy_child_statement() -> CopyStatement {
        CopyStatement {
            table: "fk_copy_children".to_string(),
            columns: vec!["parent_pid".to_string()],
            format: CopyFormat::Csv,
            header: false,
        }
    }

    fn create_copy_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
        for sql in [
            "CREATE TABLE fk_copy_parents (pid INT PRIMARY KEY)",
            "CREATE TABLE fk_copy_children (parent_pid INT REFERENCES fk_copy_parents(pid))",
            "INSERT INTO fk_copy_parents (pid) VALUES (1)",
        ] {
            cassie
                .execute_sql(session, sql, vec![])
                .expect("prepare tables");
        }
    }

    fn staged_parent_change_commit(
        name: &str,
        parent_changes: &[&str],
        concurrent_child: fn(&Cassie, &cassie::app::CassieSession) -> Result<(), String>,
    ) -> (
        Result<String, cassie::app::CassieError>,
        usize,
        usize,
        String,
    ) {
        support::use_local_storage();
        let path = support::data_dir(name);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_copy_tables(&cassie, &session);
        let writer = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        for parent_change in parent_changes {
            cassie
                .execute_sql(&session, parent_change, vec![])
                .expect("stage parent change without children");
        }
        concurrent_child(&cassie, &writer).expect("concurrent child write sees the parent");
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .map(|result| result.command);
        let parents = cassie
            .midge
            .scan_documents(&support::canonical_test_collection(
                &cassie,
                "fk_copy_parents",
            ))
            .expect("scan parents")
            .into_iter()
            .filter(|parent| parent.payload.get("pid") == Some(&serde_json::json!(1)))
            .count();
        let children = cassie
            .midge
            .scan_documents(&support::canonical_test_collection(
                &cassie,
                "fk_copy_children",
            ))
            .expect("scan children")
            .len();
        (commit, parents, children, path)
    }

    fn insert_child(cassie: &Cassie, session: &cassie::app::CassieSession) -> Result<(), String> {
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_copy_children (parent_pid) VALUES (1)",
                vec![],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn copy_child(cassie: &Cassie, session: &cassie::app::CassieSession) -> Result<(), String> {
        cassie
            .copy_from_csv_stdin(session, &copy_child_statement(), b"1\n")
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn assert_parent_change_rejected(
        commit: &Result<String, cassie::app::CassieError>,
        parents: usize,
        children: usize,
    ) {
        assert!(
            matches!(
                commit,
                Err(cassie::app::CassieError::ForeignKeyViolation { .. })
            ),
            "commit orphaning a concurrent child was not rejected with 23503: {commit:?}"
        );
        assert_eq!(
            parents, 1,
            "the referenced parent key must survive the rollback"
        );
        assert_eq!(children, 1, "the concurrently committed child must remain");
    }

    #[test]
    fn should_reject_a_parent_delete_commit_after_a_concurrent_child_insert() {
        // Arrange
        let (commit, parents, children, path) = staged_parent_change_commit(
            "foreign_key_staged_parent_delete_insert",
            &["DELETE FROM fk_copy_parents WHERE pid = 1"],
            insert_child,
        );

        // Act
        let rejected = commit.is_err();

        // Assert
        assert!(rejected, "COMMIT unexpectedly succeeded");
        assert_parent_change_rejected(&commit, parents, children);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_a_parent_delete_commit_after_a_concurrent_child_copy() {
        // Arrange
        let (commit, parents, children, path) = staged_parent_change_commit(
            "foreign_key_staged_parent_delete_copy",
            &["DELETE FROM fk_copy_parents WHERE pid = 1"],
            copy_child,
        );

        // Act
        let rejected = commit.is_err();

        // Assert
        assert!(rejected, "COMMIT unexpectedly succeeded");
        assert_parent_change_rejected(&commit, parents, children);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_a_parent_key_update_commit_after_a_concurrent_child_insert() {
        // Arrange
        let (commit, parents, children, path) = staged_parent_change_commit(
            "foreign_key_staged_parent_key_update_insert",
            &["UPDATE fk_copy_parents SET pid = 2 WHERE pid = 1"],
            insert_child,
        );

        // Act
        let rejected = commit.is_err();

        // Assert
        assert!(rejected, "COMMIT unexpectedly succeeded");
        assert_parent_change_rejected(&commit, parents, children);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_a_copy_whose_middle_row_references_a_missing_parent() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("foreign_key_copy_missing_parent");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_copy_tables(&cassie, &session);

        // Act
        let result = cassie.copy_from_csv_stdin(&session, &copy_child_statement(), b"1\n2\n1\n");

        // Assert
        let child_collection = support::canonical_test_collection(&cassie, "fk_copy_children");
        let children = cassie
            .midge
            .scan_documents(&child_collection)
            .expect("scan children");
        assert!(
            matches!(
                result,
                Err(cassie::app::CassieError::ForeignKeyViolation { .. })
            ),
            "COPY with a missing parent was not rejected: {result:?}"
        );
        assert!(children.is_empty(), "COPY applied child rows: {children:?}");

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_a_transactional_copy_commit_after_its_parent_is_deleted() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("foreign_key_transactional_copy_parent_delete");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        create_copy_tables(&cassie, &session);
        let deleter = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .copy_from_csv_stdin(&session, &copy_child_statement(), b"1\n")
            .expect("stage child copy");
        cassie
            .execute_sql(
                &deleter,
                "DELETE FROM fk_copy_parents WHERE pid = 1",
                vec![],
            )
            .expect("delete parent outside the transaction");

        // Act
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]);

        // Assert
        let child_collection = support::canonical_test_collection(&cassie, "fk_copy_children");
        let children = cassie
            .midge
            .scan_documents(&child_collection)
            .expect("scan children");
        assert!(
            matches!(
                commit,
                Err(cassie::app::CassieError::ForeignKeyViolation { .. })
            ),
            "commit of an orphaned COPY row was not rejected: {:?}",
            commit.map(|result| result.command)
        );
        assert!(
            children.is_empty(),
            "orphaned child committed: {children:?}"
        );

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
    fn should_bind_altered_check_constraint_to_declared_column_case() {
        // Arrange
        use_local_storage();
        let path = data_dir("alter_constraint_column_case");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE check_case (Age INT, v INT)", vec![])
            .expect("create check table");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE check_case ADD CONSTRAINT age_positive CHECK (age > 0)",
                vec![],
            )
            .expect("add check constraint");
        // Act
        let invalid_check = cassie.execute_sql(
            &session,
            "INSERT INTO check_case (Age, v) VALUES (-5, 1)",
            vec![],
        );

        // Assert
        assert!(invalid_check.is_err(), "case-mismatched CHECK was skipped");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_altered_check_constraint_when_existing_row_violates_it() {
        // Arrange
        use_local_storage();
        let path = data_dir("alter_check_existing_violation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE check_existing (n INT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO check_existing (n) VALUES (-5)",
                vec![],
            )
            .expect("insert violating row");

        // Act
        let added = cassie.execute_sql(
            &session,
            "ALTER TABLE check_existing ADD CONSTRAINT positive_n CHECK (n > 0)",
            vec![],
        );

        // Assert
        assert!(added.is_err(), "CHECK was added over a violating row");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_satisfy_check_constraints_when_the_value_is_explicit_null() {
        // Arrange
        use_local_storage();
        let path = data_dir("check_explicit_null");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE check_nulls (age INT CHECK (age > 0), code TEXT CHECK (code = 'x'), v INT)",
            "INSERT INTO check_nulls (age, code, v) VALUES (5, 'x', 1)",
            "CREATE TABLE check_null_existing (n INT, v INT)",
            "INSERT INTO check_null_existing (n, v) VALUES (NULL, 1)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO check_nulls (age, code, v) VALUES (NULL, NULL, 2)",
            vec![],
        );
        let updated = cassie.execute_sql(
            &session,
            "UPDATE check_nulls SET age = NULL, code = NULL WHERE v = 1",
            vec![],
        );
        let added = cassie.execute_sql(
            &session,
            "ALTER TABLE check_null_existing ADD CONSTRAINT positive_n CHECK (n > 0)",
            vec![],
        );
        let stored = cassie
            .execute_sql(
                &session,
                "SELECT age, code, v FROM check_nulls ORDER BY v",
                vec![],
            )
            .expect("select rows")
            .rows;

        // Assert
        assert!(inserted.is_ok(), "explicit NULL must satisfy the CHECKs");
        assert!(updated.is_ok(), "updating to NULL must satisfy the CHECKs");
        assert!(added.is_ok(), "an existing NULL must satisfy a new CHECK");
        assert_eq!(
            stored,
            vec![
                vec![Value::Null, Value::Null, Value::Int64(1)],
                vec![Value::Null, Value::Null, Value::Int64(2)],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_altered_primary_key_when_existing_row_has_null_key() {
        // Arrange
        use_local_storage();
        let path = data_dir("alter_primary_key_existing_null");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE primary_existing (id INT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO primary_existing (id) VALUES (NULL)",
                vec![],
            )
            .expect("insert null key");

        // Act
        let added = cassie.execute_sql(
            &session,
            "ALTER TABLE primary_existing ADD CONSTRAINT primary_existing_pk PRIMARY KEY (id)",
            vec![],
        );

        // Assert
        assert!(added.is_err(), "PRIMARY KEY was added over a null key");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bind_altered_unique_constraint_to_declared_column_case() {
        // Arrange
        use_local_storage();
        let path = data_dir("alter_unique_constraint_column_case");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_case (Age INT, v INT)",
                vec![],
            )
            .expect("create unique table");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE unique_case ADD CONSTRAINT age_unique UNIQUE (age)",
                vec![],
            )
            .expect("add unique constraint");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_case (Age, v) VALUES (5, 1)",
                vec![],
            )
            .expect("insert first unique row");

        // Act
        let duplicate_unique = cassie.execute_sql(
            &session,
            "INSERT INTO unique_case (Age, v) VALUES (5, 2)",
            vec![],
        );

        // Assert
        assert!(
            duplicate_unique.is_err(),
            "case-mismatched UNIQUE was skipped"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_altered_primary_key_writable_given_a_differently_cased_column_name() {
        // Arrange
        use_local_storage();
        let path = data_dir("alter_primary_key_column_case");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE primary_key_case (id INT, amount INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE primary_key_case ADD CONSTRAINT primary_key_case_pk PRIMARY KEY (ID)",
                vec![],
            )
            .expect("add primary key");

        // Act
        let first_insert = cassie.execute_sql(
            &session,
            "INSERT INTO primary_key_case (id, amount) VALUES (1, 5)",
            vec![],
        );
        let duplicate_insert = cassie.execute_sql(
            &session,
            "INSERT INTO primary_key_case (id, amount) VALUES (1, 6)",
            vec![],
        );

        // Assert
        assert!(
            first_insert.is_ok(),
            "case-mismatched PRIMARY KEY made the table unwritable"
        );
        assert!(
            matches!(
                duplicate_insert,
                Err(cassie::app::CassieError::UniqueViolation { .. })
            ),
            "case-mismatched PRIMARY KEY did not reject a duplicate"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_malformed_like_pattern_in_check_constraint() {
        // Arrange
        use_local_storage();
        let path = data_dir("check_like_malformed_pattern");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                r"CREATE TABLE check_like_pattern (code TEXT CHECK (code LIKE 'ab\'))",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO check_like_pattern (code) VALUES ('abc')",
            vec![],
        );

        // Assert
        let error = inserted.expect_err("malformed LIKE pattern must surface");
        assert!(
            error
                .to_string()
                .contains("LIKE pattern must not end with escape character"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

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
        assert!(matches!(
            inserted,
            Err(cassie::app::CassieError::UniqueViolation { constraint, .. })
                if constraint == cassie::catalog::local_name(&index.name)
        ));

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
        assert!(matches!(
            updated,
            Err(cassie::app::CassieError::UniqueViolation { constraint, .. })
                if constraint == cassie::catalog::local_name(&index.name)
        ));

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
    use cassie::midge::adapter::set_unique_constraint_cleanup_failure_point;

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
    fn should_reuse_unique_value_after_constraint_removal() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop_readd_unique_reservation");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE drop_readd_unique (email TEXT, CONSTRAINT email_key UNIQUE (email))",
            "INSERT INTO drop_readd_unique (email) VALUES ('a@x')",
            "ALTER TABLE drop_readd_unique DROP CONSTRAINT email_key",
            "DELETE FROM drop_readd_unique",
            "ALTER TABLE drop_readd_unique ADD CONSTRAINT email_uq UNIQUE (email)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO drop_readd_unique (email) VALUES ('a@x')",
            vec![],
        );

        // Assert
        assert!(inserted.is_ok(), "dropped constraint reservation leaked");
        let rows = cassie
            .execute_sql(&session, "SELECT email FROM drop_readd_unique", vec![])
            .expect("read rows");
        assert_eq!(rows.rows.len(), 1);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_reservation_when_primary_key_remains_after_unique_drop() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop_unique_keep_primary_reservation");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE drop_unique_keep_primary (id TEXT, CONSTRAINT id_pk PRIMARY KEY (id), CONSTRAINT id_uq UNIQUE (id))",
            "INSERT INTO drop_unique_keep_primary (id) VALUES ('same')",
            "ALTER TABLE drop_unique_keep_primary DROP CONSTRAINT id_uq",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO drop_unique_keep_primary (id) VALUES ('same')",
            vec![],
        );

        // Assert
        assert!(duplicate.is_err(), "primary key reservation was removed");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_unique_reservation_cleanup_after_interrupted_constraint_drop() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop_unique_reservation_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE drop_unique_recovery (email TEXT, CONSTRAINT email_key UNIQUE (email))",
            "INSERT INTO drop_unique_recovery (email) VALUES ('a@x')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        set_unique_constraint_cleanup_failure_point(true);

        // Act
        let interrupted = cassie.execute_sql(
            &session,
            "ALTER TABLE drop_unique_recovery DROP CONSTRAINT email_key",
            vec![],
        );
        drop(cassie);
        let recovered = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        recovered.startup().expect("replay reservation cleanup");
        let session = recovered.create_session("tester", None);
        for sql in [
            "DELETE FROM drop_unique_recovery",
            "ALTER TABLE drop_unique_recovery ADD CONSTRAINT email_uq UNIQUE (email)",
        ] {
            recovered.execute_sql(&session, sql, vec![]).expect(sql);
        }
        let inserted = recovered.execute_sql(
            &session,
            "INSERT INTO drop_unique_recovery (email) VALUES ('a@x')",
            vec![],
        );

        // Assert
        assert!(interrupted.is_err(), "failure point did not interrupt drop");
        assert!(inserted.is_ok(), "recovered drop left a stale reservation");
        let rows = recovered
            .execute_sql(&session, "SELECT email FROM drop_unique_recovery", vec![])
            .expect("read rows");
        assert_eq!(rows.rows.len(), 1);
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

    fn run_sql(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        sql: &str,
    ) -> Result<(), cassie::app::CassieError> {
        cassie.execute_sql(session, sql, vec![]).map(|_| ())
    }

    #[test]
    fn should_match_float_foreign_keys_across_all_write_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_float_numeric_equivalence");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_float_parents (id FLOAT PRIMARY KEY)",
                "CREATE TABLE fk_float_children (cid INT PRIMARY KEY, parent_id FLOAT REFERENCES fk_float_parents(id))",
                "INSERT INTO fk_float_parents VALUES (20.0)",
            ] {
                run_sql(&cassie, &session, sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
            }

            // Act
            let literal_decimal =
                run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (1, 20.0)");
            let literal_integer =
                run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (2, 20)");
            let bound_parameter = cassie.execute_sql(
                &session,
                "INSERT INTO fk_float_children VALUES (3, $1)",
                vec![Value::Float64(20.0)],
            ).map(|_| ());
            let updated_child = run_sql(
                &cassie,
                &session,
                "UPDATE fk_float_children SET parent_id = 20.0 WHERE cid = 3",
            );
            let delete_referenced_parent =
                run_sql(&cassie, &session, "DELETE FROM fk_float_parents WHERE id = 20.0");
            run_sql(&cassie, &session, "BEGIN").expect("begin transaction");
            let staged_parent = run_sql(&cassie, &session, "INSERT INTO fk_float_parents VALUES (30.0)");
            let staged_child = run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (10, 30.0)");
            let committed = run_sql(&cassie, &session, "COMMIT");
            let post_commit_child = run_sql(&cassie, &session, "INSERT INTO fk_float_children VALUES (11, 30.0)");
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT cid FROM fk_float_children ORDER BY cid",
                    vec![],
                )
                .expect("read children");

            // Assert
            literal_decimal.expect("decimal literal must match the FLOAT parent key");
            literal_integer.expect("integer literal must match the FLOAT parent key");
            bound_parameter.expect("bound parameter must match the FLOAT parent key");
            updated_child.expect("updated child key must match the FLOAT parent key");
            assert!(delete_referenced_parent
                .expect_err("referenced FLOAT parent must not be deleted")
                .to_string()
                .contains("foreign key constraint"));
            staged_parent.expect("insert parent inside transaction");
            staged_child.expect("transactional child key must match staged parent key");
            committed.expect("commit transaction");
            post_commit_child.expect("committed parent key must remain referenceable");
            assert_eq!(
                rows.rows,
                vec![
                    vec![Value::Int64(1)],
                    vec![Value::Int64(2)],
                    vec![Value::Int64(3)],
                    vec![Value::Int64(10)],
                    vec![Value::Int64(11)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

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

    #[test]
    fn should_cascade_referenced_key_update_through_a_two_level_chain() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_update_cascade_two_levels");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_chain_a (code TEXT PRIMARY KEY)",
                "CREATE TABLE fk_chain_b (code TEXT PRIMARY KEY, CONSTRAINT fk_chain_b_a FOREIGN KEY (code) REFERENCES fk_chain_a(code) ON UPDATE CASCADE)",
                "CREATE TABLE fk_chain_c (id INT PRIMARY KEY, bcode TEXT, CONSTRAINT fk_chain_c_b FOREIGN KEY (bcode) REFERENCES fk_chain_b(code) ON UPDATE CASCADE)",
                "INSERT INTO fk_chain_a VALUES ('x')",
                "INSERT INTO fk_chain_b VALUES ('x')",
                "INSERT INTO fk_chain_c VALUES (1, 'x')",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare two-level foreign key chain");
            }

            // Act
            cassie
                .execute_sql(&session, "UPDATE fk_chain_a SET code = 'y'", vec![])
                .expect("update root key");
            let grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read cascaded grandchild key");
            cassie
                .execute_sql(&session, "BEGIN", vec![])
                .expect("begin transaction");
            cassie
                .execute_sql(&session, "UPDATE fk_chain_a SET code = 'z'", vec![])
                .expect("update root key in transaction");
            let staged_grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read staged cascaded grandchild key");
            cassie
                .execute_sql(&session, "COMMIT", vec![])
                .expect("commit recursive cascade");
            let committed_grandchild = cassie
                .execute_sql(&session, "SELECT bcode FROM fk_chain_c WHERE id = 1", vec![])
                .expect("read committed cascaded grandchild key");

            // Assert
            assert_eq!(
                grandchild.rows,
                vec![vec![Value::String("y".to_string())]],
                "a cascaded child update must recurse to its own dependents"
            );
            assert_eq!(
                staged_grandchild.rows,
                vec![vec![Value::String("z".to_string())]],
                "recursive cascades must remain visible in the updating transaction"
            );
            assert_eq!(
                committed_grandchild.rows,
                vec![vec![Value::String("z".to_string())]],
                "the transaction must commit the entire recursive cascade"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_propagate_set_null_after_parent_key_update() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_set_null_two_levels");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_setnull_update_a (id INT PRIMARY KEY)",
                "CREATE TABLE fk_setnull_update_b (id INT PRIMARY KEY, code INT UNIQUE, CONSTRAINT fk_setnull_update_b_a FOREIGN KEY (code) REFERENCES fk_setnull_update_a(id) ON UPDATE SET NULL)",
                "CREATE TABLE fk_setnull_update_c (id INT PRIMARY KEY, code INT, CONSTRAINT fk_setnull_update_c_b FOREIGN KEY (code) REFERENCES fk_setnull_update_b(code) ON UPDATE SET NULL)",
                "INSERT INTO fk_setnull_update_a VALUES (1)",
                "INSERT INTO fk_setnull_update_b VALUES (10, 1)",
                "INSERT INTO fk_setnull_update_c VALUES (100, 1)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare SET NULL foreign key chains");
            }

            // Act
            cassie
                .execute_sql(
                    &session,
                    "UPDATE fk_setnull_update_a SET id = 2",
                    vec![],
                )
                .expect("update parent with SET NULL action");
            let updated_grandchild = cassie
                .execute_sql(&session, "SELECT code FROM fk_setnull_update_c", vec![])
                .expect("read update grandchild");

            // Assert
            assert_eq!(updated_grandchild.rows, vec![vec![Value::Null]]);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_propagate_set_null_after_parent_delete() {
        // Arrange
        use_local_storage();
        let path = data_dir("foreign_key_delete_set_null_two_levels");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE fk_setnull_delete_a (id INT PRIMARY KEY)",
                "CREATE TABLE fk_setnull_delete_b (id INT PRIMARY KEY, code INT UNIQUE, CONSTRAINT fk_setnull_delete_b_a FOREIGN KEY (code) REFERENCES fk_setnull_delete_a(id) ON DELETE SET NULL)",
                "CREATE TABLE fk_setnull_delete_c (id INT PRIMARY KEY, code INT, CONSTRAINT fk_setnull_delete_c_b FOREIGN KEY (code) REFERENCES fk_setnull_delete_b(code) ON UPDATE SET NULL)",
                "INSERT INTO fk_setnull_delete_a VALUES (1)",
                "INSERT INTO fk_setnull_delete_b VALUES (10, 1)",
                "INSERT INTO fk_setnull_delete_c VALUES (100, 1)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("prepare SET NULL foreign key chain");
            }

            // Act
            cassie
                .execute_sql(&session, "DELETE FROM fk_setnull_delete_a", vec![])
                .expect("delete parent with SET NULL action");
            let grandchild = cassie
                .execute_sql(&session, "SELECT code FROM fk_setnull_delete_c", vec![])
                .expect("read delete grandchild");

            // Assert
            assert_eq!(grandchild.rows, vec![vec![Value::Null]]);

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
        crate::support_temp_dirs::sweep_stale_once();
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

    #[test]
    fn should_reject_an_invalid_timestamp_string_on_insert() {
        // Arrange
        use_local_storage();
        let path = data_dir("insert_invalid_timestamp");
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
                    "CREATE TABLE insert_invalid_timestamp (id INT, ts TIMESTAMP)",
                    vec![],
                )
                .unwrap();

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO insert_invalid_timestamp (id, ts) VALUES (1, 'not-a-timestamp')",
                vec![],
            );

            // Assert
            assert!(
                inserted.is_err(),
                "an unparseable TIMESTAMP string must be rejected on write"
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cast_a_timestamp_string_to_its_canonical_utc_form() {
        // Arrange
        use_local_storage();
        let path = data_dir("cast_timestamp_canonical");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let cast_offset = cassie
                .execute_sql(
                    &session,
                    "SELECT CAST('2024-01-01T09:00:00+02:00' AS TIMESTAMP) AS ts",
                    vec![],
                )
                .unwrap();
            let cast_invalid = cassie.execute_sql(
                &session,
                "SELECT CAST('not-a-timestamp' AS TIMESTAMP) AS ts",
                vec![],
            );

            // Assert
            assert_eq!(
                cast_offset.rows[0][0],
                Value::String("2024-01-01T07:00:00.000000Z".to_string())
            );
            assert!(
                cast_invalid.is_err(),
                "casting an invalid string to TIMESTAMP must fail"
            );

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
    use cassie::app::{Cassie, CassieSession};
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

    fn setup_search_path_rollback_tables(cassie: &Cassie, session: &CassieSession) {
        cassie
            .execute_sql(session, "CREATE SCHEMA other_schema", vec![])
            .unwrap();
        cassie
            .execute_sql(
                session,
                "CREATE TABLE public.transaction_search_path (value INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "CREATE TABLE other_schema.transaction_search_path (value INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "INSERT INTO public.transaction_search_path VALUES (1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                session,
                "INSERT INTO other_schema.transaction_search_path VALUES (2)",
                vec![],
            )
            .unwrap();
    }

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
    fn should_restore_search_path_after_transaction_rollback() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction_rollback_search_path");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            setup_search_path_rollback_tables(&cassie, &session);
            cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
            cassie
                .execute_sql(
                    &session,
                    "SET application_name TO 'before_savepoint'",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&session, "SAVEPOINT before_setting_change", vec![])
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "SET application_name TO 'after_savepoint'",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&session, "SET search_path TO other_schema", vec![])
                .unwrap();

            // Act
            cassie
                .execute_sql(
                    &session,
                    "ROLLBACK TO SAVEPOINT before_setting_change",
                    vec![],
                )
                .unwrap();
            let savepoint_path = cassie
                .execute_sql(&session, "SHOW search_path", vec![])
                .unwrap();
            let savepoint_application_name = cassie
                .execute_sql(&session, "SHOW application_name", vec![])
                .unwrap();
            cassie
                .execute_sql(&session, "SET search_path TO other_schema", vec![])
                .unwrap();

            cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();
            let search_path = cassie
                .execute_sql(&session, "SHOW search_path", vec![])
                .unwrap();
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT value FROM transaction_search_path",
                    vec![],
                )
                .unwrap();
            let application_name = cassie
                .execute_sql(&session, "SHOW application_name", vec![])
                .unwrap();

            // Assert
            assert_eq!(
                savepoint_path.rows,
                vec![vec![Value::String("public".to_string())]]
            );
            assert_eq!(
                savepoint_application_name.rows,
                vec![vec![Value::String("before_savepoint".to_string())]]
            );
            assert_eq!(
                search_path.rows,
                vec![vec![Value::String("public".to_string())]]
            );
            assert_eq!(
                application_name.rows,
                vec![vec![Value::String(String::new())]]
            );
            assert_eq!(selected.rows, vec![vec![Value::Int64(1)]]);

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
    fn should_not_apply_column_defaults_during_update_or_upsert() {
        // Arrange
        use_local_storage();
        let path = data_dir("update_does_not_apply_defaults");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE update_default_rows (id INT PRIMARY KEY, a INT, b INT)",
                "CREATE SEQUENCE update_default_seq",
                "INSERT INTO update_default_rows (id, a) VALUES (1, 1)",
                "ALTER TABLE update_default_rows ALTER COLUMN b SET DEFAULT nextval('update_default_seq')",
                "UPDATE update_default_rows SET a = 2 WHERE id = 1",
                "INSERT INTO update_default_rows (id, a) VALUES (2, 2)",
                "INSERT INTO update_default_rows (id, a) VALUES (1, 3) ON CONFLICT (id) DO UPDATE SET a = excluded.a",
                "INSERT INTO update_default_rows (id, a) VALUES (3, 3)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect(sql);
            }

            // Act
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT id, a, b FROM update_default_rows ORDER BY id",
                    vec![],
                )
                .expect("read rows");

            // Assert
            assert_eq!(
                rows.rows,
                vec![
                    vec![Value::Int64(1), Value::Int64(3), Value::Null],
                    vec![Value::Int64(2), Value::Int64(2), Value::Int64(1)],
                    vec![Value::Int64(3), Value::Int64(3), Value::Int64(2)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_not_apply_column_defaults_during_foreign_key_cascade_updates() {
        // Arrange
        use_local_storage();
        let path = data_dir("cascade_update_does_not_apply_defaults");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE default_cascade_parents (id INT PRIMARY KEY)",
                "CREATE SEQUENCE cascade_update_default_seq",
                "CREATE TABLE default_cascade_children (parent_id INT, marker INT, CONSTRAINT default_cascade_children_fkey FOREIGN KEY (parent_id) REFERENCES default_cascade_parents(id) ON UPDATE CASCADE)",
                "INSERT INTO default_cascade_parents VALUES (1), (2)",
                "INSERT INTO default_cascade_children (parent_id) VALUES (1)",
                "ALTER TABLE default_cascade_children ALTER COLUMN marker SET DEFAULT nextval('cascade_update_default_seq')",
                "UPDATE default_cascade_parents SET id = 3 WHERE id = 1",
                "INSERT INTO default_cascade_children (parent_id) VALUES (2)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect(sql);
            }

            // Act
            let rows = cassie
                .execute_sql(
                    &session,
                    "SELECT parent_id, marker FROM default_cascade_children ORDER BY parent_id",
                    vec![],
                )
                .expect("read cascaded child rows");

            // Assert
            assert_eq!(
                rows.rows,
                vec![
                    vec![Value::Int64(2), Value::Int64(1)],
                    vec![Value::Int64(3), Value::Null],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_non_finite_float_values_on_write_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("non_finite_write_paths");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE non_finite_writes (item_id INT PRIMARY KEY, x FLOAT)",
                "CREATE TABLE non_finite_writes_copy (item_id INT, x FLOAT)",
                "INSERT INTO non_finite_writes (item_id, x) VALUES (1, 5.5)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect(sql);
            }
            let statements = [
                "UPDATE non_finite_writes SET x = $1 WHERE item_id = 1",
                "UPDATE non_finite_writes SET x = x * $1 WHERE item_id = 1",
                "INSERT INTO non_finite_writes (item_id, x) VALUES (2, $1)",
                "INSERT INTO non_finite_writes_copy (item_id, x) SELECT item_id, x * $1 FROM non_finite_writes",
                "INSERT INTO non_finite_writes (item_id, x) VALUES (1, 2.5) \
                 ON CONFLICT (item_id) DO UPDATE SET x = $1",
            ];

            // Act
            let mut unexpected = Vec::new();
            for sql in statements {
                for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    match cassie.execute_sql(&session, sql, vec![Value::Float64(parameter)]) {
                        Err(error)
                            if error
                                .to_string()
                                .contains("stored float values must be finite") => {}
                        outcome => unexpected.push(format!("{sql} [{parameter}]: {outcome:?}")),
                    }
                }
            }
            let stored = cassie
                .execute_sql(&session, "SELECT item_id, x FROM non_finite_writes", vec![])
                .expect("select stored rows");
            let copied = cassie
                .execute_sql(&session, "SELECT item_id, x FROM non_finite_writes_copy", vec![])
                .expect("select copied rows");

            // Assert
            assert!(
                unexpected.is_empty(),
                "non-finite float values must be rejected as non-finite: {unexpected:#?}"
            );
            assert_eq!(
                stored.rows,
                vec![vec![Value::Int64(1), Value::Float64(5.5)]]
            );
            assert!(copied.rows.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

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
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir().join(format!("cassie-upsert-{name}-{}", Uuid::new_v4()))
    }

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    #[test]
    fn should_upsert_when_conflict_key_has_a_noncanonical_input_shape() {
        // Arrange
        use_local_storage();
        let path = data_dir("canonical_conflict_keys");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie.execute_sql(&session, "CREATE TABLE canonical_float (k FLOAT UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "CREATE TABLE canonical_date (k DATE UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "CREATE TABLE canonical_timestamp (k TIMESTAMP UNIQUE, v INT)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1.0, 1)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-01-01', 1)", vec![]).unwrap();
            cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01T00:00:00Z', 1)", vec![]).unwrap();

            // Act
            let float_update = cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1, 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);
            let date_update = cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-1-1', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);
            let timestamp_update = cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01 00:00:00', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v", vec![]);

            // Assert
            assert!(float_update.is_ok(), "FLOAT conflict should update");
            assert!(date_update.is_ok(), "DATE conflict should update");
            assert!(timestamp_update.is_ok(), "TIMESTAMP conflict should update");
            for table in ["canonical_float", "canonical_date", "canonical_timestamp"] {
                let rows = cassie
                    .execute_sql(&session, &format!("SELECT v FROM {table}"), vec![])
                    .unwrap();
                assert_eq!(rows.rows, vec![vec![Value::Int64(2)]], "table {table}");
            }
            let float_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_float VALUES (1, 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(float_nothing.is_ok(), "FLOAT DO NOTHING should suppress conflict");
            let date_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_date VALUES ('2024-1-1', 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(date_nothing.is_ok(), "DATE DO NOTHING should suppress conflict");
            let timestamp_nothing = cassie.execute_sql(&session, "INSERT INTO canonical_timestamp VALUES ('2024-01-01 00:00:00', 3) ON CONFLICT (k) DO NOTHING", vec![]);
            assert!(timestamp_nothing.is_ok(), "TIMESTAMP DO NOTHING should suppress conflict");
            assert_eq!(float_nothing.unwrap().command, "INSERT 0 0");
            assert_eq!(date_nothing.unwrap().command, "INSERT 0 0");
            assert_eq!(timestamp_nothing.unwrap().command, "INSERT 0 0");
            for table in ["canonical_float", "canonical_date", "canonical_timestamp"] {
                let rows = cassie
                    .execute_sql(&session, &format!("SELECT v FROM {table}"), vec![])
                    .unwrap();
                assert_eq!(rows.rows, vec![vec![Value::Int64(2)]], "table {table}");
            }

            let _ = std::fs::remove_dir_all(path);
        });
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
                Value::String("nextval('postgres.public.order_ids'::regclass)".to_string()),
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
            Value::String("nextval('postgres.public.order_ids'::regclass)".to_string()),
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
    fn should_register_serial_sequence_in_schema_of_qualified_table() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial_schema_qualified_table");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", Some("postgres".to_string()));
            execute_statement(&cassie, &session, "CREATE SCHEMA ledger");

            // Act
            execute_statement(
                &cassie,
                &session,
                "CREATE TABLE ledger.entries (entry_id SERIAL PRIMARY KEY, label TEXT)",
            );
            execute_statement(
                &cassie,
                &session,
                "INSERT INTO ledger.entries (label) VALUES ('one')",
            );
            let sequence_names = cassie
                .catalog
                .list_sequences()
                .into_iter()
                .map(|sequence| sequence.name)
                .collect::<Vec<_>>();
            let rows = query_rows(
                &cassie,
                &session,
                "SELECT entry_id, label FROM ledger.entries",
            );
            execute_statement(&cassie, &session, "DROP TABLE ledger.entries");
            let remaining_sequences = cassie.catalog.list_sequences();

            // Assert
            assert_eq!(
                sequence_names,
                vec!["postgres.ledger.entries_entry_id_seq".to_string()]
            );
            assert_eq!(
                rows,
                vec![vec![Value::Int64(1), Value::String("one".to_string())]]
            );
            assert!(remaining_sequences.is_empty());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_qualify_serial_sequence_for_unqualified_table_in_search_path_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial_search_path_schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let public_session = cassie.create_session("tester", Some("postgres".to_string()));
            execute_statement(&cassie, &public_session, "CREATE SCHEMA reporting");
            execute_statement(
                &cassie,
                &public_session,
                "CREATE TABLE metrics (metric_id SERIAL, label TEXT)",
            );
            let reporting_session = cassie.create_session("tester", Some("postgres".to_string()));
            execute_statement(&cassie, &reporting_session, "SET search_path = reporting");

            // Act
            execute_statement(
                &cassie,
                &reporting_session,
                "CREATE TABLE metrics (metric_id SERIAL, label TEXT)",
            );
            execute_statement(
                &cassie,
                &public_session,
                "INSERT INTO metrics (label) VALUES ('public-one')",
            );
            execute_statement(
                &cassie,
                &reporting_session,
                "INSERT INTO metrics (label) VALUES ('reporting-one')",
            );
            execute_statement(
                &cassie,
                &public_session,
                "ALTER SCHEMA reporting RENAME TO analytics",
            );
            let renamed_insert = cassie.execute_sql(
                &public_session,
                "INSERT INTO analytics.metrics (label) VALUES ('analytics-two')",
                vec![],
            );
            let sequence_names = cassie
                .catalog
                .list_sequences()
                .into_iter()
                .map(|sequence| sequence.name)
                .collect::<Vec<_>>();
            let analytics_rows = query_rows(
                &cassie,
                &public_session,
                "SELECT metric_id, label FROM analytics.metrics ORDER BY metric_id",
            );
            let public_rows = query_rows(
                &cassie,
                &public_session,
                "SELECT metric_id, label FROM public.metrics ORDER BY metric_id",
            );

            // Assert
            assert!(
                sequence_names.contains(&"postgres.analytics.metrics_metric_id_seq".to_string()),
                "sequence was not qualified with the resolved schema: {sequence_names:?}"
            );
            assert!(
                renamed_insert.is_ok(),
                "insert after schema rename failed: {renamed_insert:?}"
            );
            assert_eq!(
                analytics_rows,
                vec![
                    vec![Value::Int64(1), Value::String("reporting-one".to_string())],
                    vec![Value::Int64(2), Value::String("analytics-two".to_string())],
                ]
            );
            assert_eq!(
                public_rows,
                vec![vec![
                    Value::Int64(1),
                    Value::String("public-one".to_string())
                ]]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_serial_default_working_after_renaming_its_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial_schema_rename");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", Some("postgres".to_string()));
            execute_statement(&cassie, &session, "CREATE SCHEMA billing");
            execute_statement(
                &cassie,
                &session,
                "CREATE TABLE billing.invoices (invoice_id SERIAL PRIMARY KEY, label TEXT)",
            );
            execute_statement(
                &cassie,
                &session,
                "INSERT INTO billing.invoices (label) VALUES ('one')",
            );
            execute_statement(
                &cassie,
                &session,
                "ALTER SCHEMA billing RENAME TO invoicing",
            );

            // Act
            let inserted = cassie.execute_sql(
                &session,
                "INSERT INTO invoicing.invoices (label) VALUES ('two')",
                vec![],
            );
            let rows = query_rows(
                &cassie,
                &session,
                "SELECT invoice_id, label FROM invoicing.invoices ORDER BY invoice_id",
            );

            // Assert
            assert!(
                inserted.is_ok(),
                "insert after schema rename failed: {inserted:?}"
            );
            assert_eq!(
                rows,
                vec![
                    vec![Value::Int64(1), Value::String("one".to_string())],
                    vec![Value::Int64(2), Value::String("two".to_string())],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_release_serial_sequence_when_dropping_its_table() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial_sequence_table_drop");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE SCHEMA sx",
                "CREATE TABLE sx.t (k SERIAL, v INT)",
                "INSERT INTO sx.t (v) VALUES (1)",
                "DROP TABLE sx.t",
                "DROP SCHEMA sx",
                "CREATE SCHEMA sx",
                "CREATE TABLE sx.t (k SERIAL, v INT)",
                "INSERT INTO sx.t (v) VALUES (2)",
            ] {
                execute_statement(&cassie, &session, sql);
            }

            // Act
            let rows = query_rows(&cassie, &session, "SELECT k FROM sx.t");

            // Assert
            assert_eq!(rows, vec![vec![Value::Int64(1)]]);
            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_rename_serial_sequence_when_renaming_its_table() {
        // Arrange
        use_local_storage();
        let path = data_dir("serial_sequence_table_rename");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE t (k SERIAL, v INT)",
                "INSERT INTO t (v) VALUES (1)",
                "ALTER TABLE t RENAME TO t2",
                "INSERT INTO t2 (v) VALUES (2)",
                "CREATE TABLE t (k SERIAL, v INT)",
                "INSERT INTO t (v) VALUES (3)",
            ] {
                execute_statement(&cassie, &session, sql);
            }

            // Act
            let renamed_rows = query_rows(&cassie, &session, "SELECT k FROM t2 ORDER BY k");
            let recreated_rows = query_rows(&cassie, &session, "SELECT k FROM t");

            // Assert
            assert_eq!(
                renamed_rows,
                vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
            );
            assert_eq!(recreated_rows, vec![vec![Value::Int64(1)]]);
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
    #[test]
    fn should_bind_bare_explicit_sequence_defaults_to_table_schema() {
        // Arrange
        use_local_storage();
        let path = data_dir("explicit-sequence-default-schema");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE SCHEMA aa",
                "CREATE SCHEMA bb",
                "CREATE SEQUENCE aa.s",
                "CREATE SEQUENCE bb.s",
                "CREATE SEQUENCE aa.altered",
                "CREATE SEQUENCE bb.altered",
                "CREATE TABLE bb.t (v INT, n INT DEFAULT nextval('s'))",
                "CREATE TABLE bb.altered_t (v INT, n INT)",
                "ALTER TABLE bb.altered_t ALTER COLUMN n SET DEFAULT nextval('altered')",
                "INSERT INTO bb.t (v) VALUES (1), (2)",
                "INSERT INTO bb.altered_t (v) VALUES (1), (2)",
                "DROP SEQUENCE aa.s",
                "DROP SEQUENCE aa.altered",
                "INSERT INTO bb.t (v) VALUES (3), (4)",
                "INSERT INTO bb.altered_t (v) VALUES (3), (4)",
            ] {
                execute_statement(&cassie, &session, sql);
            }

            // Act
            let rows = query_rows(&cassie, &session, "SELECT v, n FROM bb.t ORDER BY v");
            let altered_rows = query_rows(
                &cassie,
                &session,
                "SELECT v, n FROM bb.altered_t ORDER BY v",
            );

            // Assert
            let expected_rows = vec![
                vec![Value::Int64(1), Value::Int64(1)],
                vec![Value::Int64(2), Value::Int64(2)],
                vec![Value::Int64(3), Value::Int64(3)],
                vec![Value::Int64(4), Value::Int64(4)],
            ];
            assert_eq!(altered_rows, expected_rows);
            assert_eq!(rows, expected_rows);

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

mod unique_ddl_reservations {
    use cassie::app::Cassie;
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::Value;
    use std::collections::HashSet;

    use super::support_sql as support;

    #[test]
    fn should_not_resolve_a_renamed_column_alias_after_adding_its_old_name() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("renamed_column_alias_reuse");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE renamed_column_alias_reuse (a INT, b INT)",
            "INSERT INTO renamed_column_alias_reuse (a, b) VALUES (1, 42)",
            "ALTER TABLE renamed_column_alias_reuse RENAME COLUMN b TO c",
            "ALTER TABLE renamed_column_alias_reuse ADD COLUMN b TEXT",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let explicit = cassie
            .execute_sql(
                &session,
                "SELECT a, b, c FROM renamed_column_alias_reuse",
                vec![],
            )
            .expect("select explicit columns");
        let wildcard = cassie
            .execute_sql(&session, "SELECT * FROM renamed_column_alias_reuse", vec![])
            .expect("select wildcard columns");
        let stale_alias = cassie
            .execute_sql(
                &session,
                "SELECT a FROM renamed_column_alias_reuse WHERE b = '42'",
                vec![],
            )
            .expect("filter by newly added column");

        // Assert
        assert_eq!(
            explicit.rows,
            vec![vec![Value::Int64(1), Value::Null, Value::Int64(42)]]
        );
        assert_eq!(wildcard.rows.len(), 1);
        let column_position = |name: &str| {
            wildcard
                .columns
                .iter()
                .position(|column| column.name == name)
                .expect("wildcard column")
        };
        assert_eq!(wildcard.rows[0][column_position("a")], Value::Int64(1));
        assert_eq!(wildcard.rows[0][column_position("b")], Value::Null);
        assert_eq!(wildcard.rows[0][column_position("c")], Value::Int64(42));
        assert!(stale_alias.rows.is_empty());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_delete_unique_index_storage_when_index_is_dropped() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("drop_unique_index_storage");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE drop_unique_index_storage (id TEXT PRIMARY KEY, email TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO drop_unique_index_storage (id, email) VALUES ('r1', 'a@example.com')",
                vec![],
            )
            .expect("insert unique value");
        let keys_before_index = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan before index")
            .into_iter()
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX drop_unique_index_storage_email ON drop_unique_index_storage (email)",
                vec![],
            )
            .expect("create unique index");
        let keys_after_index = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan after index")
            .into_iter()
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        let index_keys = keys_after_index
            .difference(&keys_before_index)
            .cloned()
            .collect::<HashSet<_>>();
        assert!(
            !index_keys.is_empty(),
            "unique index should create storage entries"
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "DROP INDEX drop_unique_index_storage_email ON drop_unique_index_storage",
                vec![],
            )
            .expect("drop unique index");
        let keys_after_drop = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan after drop")
            .into_iter()
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();

        // Assert
        assert!(
            index_keys.is_disjoint(&keys_after_drop),
            "unique index storage entries remained after DROP INDEX: {:x?}",
            index_keys
                .intersection(&keys_after_drop)
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_not_restore_dropped_column_values_after_readding_the_name() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("dropped_column_value_resurrection");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE dropped_column_values (id TEXT PRIMARY KEY, note TEXT, renamed_source TEXT)",
            "INSERT INTO dropped_column_values (id, note, renamed_source) VALUES ('r1', 'secret', 'renamed secret')",
            "ALTER TABLE dropped_column_values DROP COLUMN note",
            "ALTER TABLE dropped_column_values ADD COLUMN note TEXT",
            "ALTER TABLE dropped_column_values RENAME COLUMN renamed_source TO renamed_current",
            "ALTER TABLE dropped_column_values DROP COLUMN renamed_current",
            "ALTER TABLE dropped_column_values ADD COLUMN renamed_source TEXT",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let explicit = cassie
            .execute_sql(
                &session,
                "SELECT id, note, renamed_source FROM dropped_column_values",
                vec![],
            )
            .expect("select re-added columns");
        let wildcard = cassie
            .execute_sql(&session, "SELECT * FROM dropped_column_values", vec![])
            .expect("select all re-added columns");
        let predicate = cassie
            .execute_sql(
                &session,
                "SELECT id FROM dropped_column_values WHERE note = 'secret' OR renamed_source = 'renamed secret'",
                vec![],
            )
            .expect("filter re-added columns");

        // Assert
        assert_eq!(
            explicit.rows,
            vec![vec![
                Value::String("r1".to_string()),
                Value::Null,
                Value::Null,
            ]]
        );
        assert_eq!(wildcard.rows, explicit.rows);
        assert!(predicate.rows.is_empty());

        drop(cassie);
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        reopened.startup().expect("restart Cassie");
        let reopened_session = reopened.create_session("tester", None);
        let after_restart = reopened
            .execute_sql(
                &reopened_session,
                "SELECT id, note, renamed_source FROM dropped_column_values",
                vec![],
            )
            .expect("select re-added columns after restart");
        assert_eq!(after_restart.rows, explicit.rows);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_backfill_float_reservations_after_adding_unique_constraint() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("alter_unique_constraint_float_backfill");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE alter_unique_constraint_float (k FLOAT, v INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO alter_unique_constraint_float (k, v) VALUES ($1, 1)",
                vec![Value::Float64(1.0)],
            )
            .expect("insert preexisting row");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE alter_unique_constraint_float ADD CONSTRAINT alter_unique_constraint_float_k_unique UNIQUE (k)",
                vec![],
            )
            .expect("add unique constraint");

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO alter_unique_constraint_float (k, v) VALUES (1, 2)",
            vec![],
        );

        // Assert
        assert!(
            duplicate.is_err(),
            "duplicate insert unexpectedly succeeded"
        );
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT k, v FROM alter_unique_constraint_float ORDER BY v",
                vec![],
            )
            .expect("read rows");
        assert_eq!(rows.rows, vec![vec![Value::Float64(1.0), Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_backfill_timestamp_reservations_after_creating_unique_index() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("create_unique_index_timestamp_backfill");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE create_unique_index_timestamp (at TIMESTAMP, v INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO create_unique_index_timestamp (at, v) VALUES ('2024-01-01T00:00:00Z', 1)",
                vec![],
            )
            .expect("insert preexisting row");
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX create_unique_index_timestamp_at_uq ON create_unique_index_timestamp (at)",
                vec![],
            )
            .expect("create unique index");

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO create_unique_index_timestamp (at, v) VALUES ('2024-01-01 00:00:00', 2)",
            vec![],
        );

        // Assert
        assert!(
            duplicate.is_err(),
            "duplicate insert unexpectedly succeeded"
        );
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT at, v FROM create_unique_index_timestamp ORDER BY v",
                vec![],
            )
            .expect("read rows");
        assert_eq!(
            rows.rows,
            vec![vec![
                Value::String("2024-01-01T00:00:00.000000Z".to_string()),
                Value::Int64(1)
            ]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_unique_constraint_when_preexisting_rows_duplicate() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("alter_unique_constraint_existing_duplicates");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE alter_unique_constraint_duplicates (k FLOAT, v INT)",
            "INSERT INTO alter_unique_constraint_duplicates (k, v) VALUES (1.0, 1)",
            "INSERT INTO alter_unique_constraint_duplicates (k, v) VALUES (1, 2)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let added = cassie.execute_sql(
            &session,
            "ALTER TABLE alter_unique_constraint_duplicates ADD CONSTRAINT alter_unique_constraint_duplicates_k_unique UNIQUE (k)",
            vec![],
        );

        // Assert
        assert!(
            added.is_err(),
            "constraint creation accepted duplicate rows"
        );
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT k, v FROM alter_unique_constraint_duplicates ORDER BY v",
                vec![],
            )
            .expect("read existing rows");
        assert_eq!(rows.rows.len(), 2);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_unique_index_when_preexisting_rows_duplicate() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("create_unique_index_existing_duplicates");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE create_unique_index_duplicates (at TIMESTAMP, v INT)",
            "INSERT INTO create_unique_index_duplicates (at, v) VALUES ('2024-01-01T00:00:00Z', 1)",
            "INSERT INTO create_unique_index_duplicates (at, v) VALUES ('2024-01-01 00:00:00', 2)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let created = cassie.execute_sql(
            &session,
            "CREATE UNIQUE INDEX create_unique_index_duplicates_at_uq ON create_unique_index_duplicates (at)",
            vec![],
        );

        // Assert
        assert!(created.is_err(), "unique index accepted duplicate rows");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT at, v FROM create_unique_index_duplicates ORDER BY v",
                vec![],
            )
            .expect("read existing rows");
        assert_eq!(rows.rows.len(), 2);
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/unique_reservations.rs.
mod unique_reservations {
    use cassie::app::Cassie;
    use cassie::midge::adapter::{
        document_write_failure_point_test_guard, set_document_write_failure_point,
        set_field_rename_failure_point, DocumentWriteFailurePoint,
    };
    use cassie::types::Value;

    use super::support_sql as support;

    #[test]
    fn should_allow_duplicate_keys_outside_partial_unique_index_predicate() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("partial_unique_index_predicate");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE partial_unique_active (email TEXT, active BOOLEAN)",
            "CREATE UNIQUE INDEX partial_unique_active_email ON partial_unique_active (email) WHERE active",
            "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', true)",
            "CREATE TABLE partial_unique_inactive (email TEXT, active BOOLEAN)",
            "CREATE UNIQUE INDEX partial_unique_inactive_email ON partial_unique_inactive (email) WHERE active",
            "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
            "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE partial_unique_backfill (email TEXT, active BOOLEAN)",
                vec![],
            )
            .expect("create table for partial unique index backfill");
        for _ in 0..2 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO partial_unique_backfill (email, active) VALUES ('c@x', false)",
                    vec![],
                )
                .expect("insert duplicate rows outside future index predicate");
        }

        // Act
        let inactive_with_active_key = cassie.execute_sql(
            &session,
            "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', false)",
            vec![],
        );
        let repeated_inactive_key = cassie.execute_sql(
            &session,
            "INSERT INTO partial_unique_inactive (email, active) VALUES ('b@x', false)",
            vec![],
        );
        let duplicate_active_key = cassie.execute_sql(
            &session,
            "INSERT INTO partial_unique_active (email, active) VALUES ('a@x', true)",
            vec![],
        );
        let activate_duplicate_key = cassie.execute_sql(
            &session,
            "UPDATE partial_unique_active SET active = true WHERE active = false",
            vec![],
        );
        let active_rows = cassie
            .execute_sql(&session, "SELECT email FROM partial_unique_active", vec![])
            .expect("read rows with one active match");
        let inactive_rows = cassie
            .execute_sql(
                &session,
                "SELECT email FROM partial_unique_inactive",
                vec![],
            )
            .expect("read rows outside predicate");
        let partial_index_creation = cassie.execute_sql(
            &session,
            "CREATE UNIQUE INDEX partial_unique_backfill_email ON partial_unique_backfill (email) WHERE active",
            vec![],
        );

        // Assert
        assert!(
            inactive_with_active_key.is_ok(),
            "a row outside the predicate may share a key with an indexed row"
        );
        assert!(
            repeated_inactive_key.is_ok(),
            "rows outside the predicate may repeat a key"
        );
        assert!(
            duplicate_active_key.is_err(),
            "rows matching the predicate must still enforce uniqueness"
        );
        assert!(
            activate_duplicate_key.is_err(),
            "an update entering the predicate must enforce uniqueness"
        );
        assert_eq!(active_rows.rows.len(), 2);
        assert_eq!(inactive_rows.rows.len(), 3);
        assert!(
            partial_index_creation.is_ok(),
            "index backfill ignores duplicate rows outside its predicate"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_allow_distinct_unique_columns_with_colliding_legacy_key_bytes() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_field_value_collision");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_reservation_field_value_collision (id TEXT PRIMARY KEY, a BIGINT UNIQUE, a0ab TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_reservation_field_value_collision (id, a) VALUES ('r1', -2206112389093773006)",
                vec![],
            )
            .expect("insert first unique value");

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_field_value_collision (id, a0ab) VALUES ('r2', 'xyz12')",
            vec![],
        );

        // Assert
        assert!(inserted.is_ok(), "distinct unique columns must not collide");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_whole_number_float_duplicates_in_unique_reservations() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_whole_float");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_reservation_whole_float (price FLOAT UNIQUE)",
            "INSERT INTO unique_reservation_whole_float (price) VALUES (5.0)",
            "INSERT INTO unique_reservation_whole_float (price) VALUES (7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let integer_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_whole_float (price) VALUES (5)",
            vec![],
        );
        let float_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_whole_float (price) VALUES (7.0)",
            vec![],
        );

        // Assert
        assert!(integer_duplicate.is_err(), "{integer_duplicate:?}");
        assert!(float_duplicate.is_err(), "{float_duplicate:?}");
        let _ = std::fs::remove_dir_all(path);
    }

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

    #[test]
    fn should_commit_transactional_unique_value_reuse_after_delete() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_transaction_delete_insert");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_reservation_transaction_delete_insert (email TEXT UNIQUE)",
                vec![],
            )
            .expect("create table");
        cassie
            .midge
            .put_document(
                "postgres.public.unique_reservation_transaction_delete_insert",
                Some("ffffffff-ffff-4fff-bfff-ffffffffffff".to_string()),
                serde_json::json!({"email": "reuse@example.com"}),
            )
            .expect("seed row with lexicographically maximal generated-ID shape");

        // Act
        for sql in [
            "BEGIN",
            "DELETE FROM unique_reservation_transaction_delete_insert WHERE email = 'reuse@example.com'",
            "INSERT INTO unique_reservation_transaction_delete_insert (email) VALUES ('reuse@example.com')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]);

        // Assert
        assert!(commit.is_ok(), "transactional value reuse failed");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT email FROM unique_reservation_transaction_delete_insert",
                vec![],
            )
            .expect("read committed row");
        assert_eq!(
            rows.rows,
            vec![vec![Value::String("reuse@example.com".to_string())]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_commit_transactional_unique_value_rotation_across_updates() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_transaction_rotation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_rotation (id INT PRIMARY KEY, code TEXT UNIQUE)",
            "INSERT INTO unique_rotation (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        for sql in [
            "BEGIN",
            "UPDATE unique_rotation SET code = 'tmp' WHERE id = 1",
            "UPDATE unique_rotation SET code = 'a' WHERE id = 2",
            "UPDATE unique_rotation SET code = 'b' WHERE id = 1",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        let rows_in_transaction = cassie
            .execute_sql(
                &session,
                "SELECT id, code FROM unique_rotation ORDER BY id",
                vec![],
            )
            .expect("read final transaction state");
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
        assert!(commit.is_ok(), "valid unique rotation must commit");
        let duplicate_b = cassie.execute_sql(
            &session,
            "UPDATE unique_rotation SET code = 'b' WHERE id = 3",
            vec![],
        );
        let duplicate_a = cassie.execute_sql(
            &session,
            "UPDATE unique_rotation SET code = 'a' WHERE id = 3",
            vec![],
        );
        let rows_after_commit = cassie
            .execute_sql(
                &session,
                "SELECT id, code FROM unique_rotation ORDER BY id",
                vec![],
            )
            .expect("read committed rows");

        // Assert
        assert_eq!(
            rows_in_transaction.rows,
            vec![
                vec![Value::Int64(1), Value::String("b".to_string())],
                vec![Value::Int64(2), Value::String("a".to_string())],
                vec![Value::Int64(3), Value::String("c".to_string())],
            ]
        );
        assert!(
            duplicate_b.is_err(),
            "rotation must retain the 'b' reservation"
        );
        assert!(
            duplicate_a.is_err(),
            "rotation must retain the 'a' reservation"
        );
        assert_eq!(rows_after_commit.rows, rows_in_transaction.rows);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_commit_transactional_unique_index_value_rotation_across_updates() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_index_transaction_rotation");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_index_rotation (id INT PRIMARY KEY, code TEXT)",
            "CREATE UNIQUE INDEX unique_index_rotation_code ON unique_index_rotation (code)",
            "INSERT INTO unique_index_rotation (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        for sql in [
            "BEGIN",
            "UPDATE unique_index_rotation SET code = 'tmp' WHERE id = 1",
            "UPDATE unique_index_rotation SET code = 'a' WHERE id = 2",
            "UPDATE unique_index_rotation SET code = 'b' WHERE id = 1",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        let rows_in_transaction = cassie
            .execute_sql(
                &session,
                "SELECT id, code FROM unique_index_rotation ORDER BY id",
                vec![],
            )
            .expect("read final transaction state");
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
        assert!(commit.is_ok(), "valid unique-index rotation must commit");
        let duplicate_b = cassie.execute_sql(
            &session,
            "UPDATE unique_index_rotation SET code = 'b' WHERE id = 3",
            vec![],
        );
        let duplicate_a = cassie.execute_sql(
            &session,
            "UPDATE unique_index_rotation SET code = 'a' WHERE id = 3",
            vec![],
        );
        let rows_after_commit = cassie
            .execute_sql(
                &session,
                "SELECT id, code FROM unique_index_rotation ORDER BY id",
                vec![],
            )
            .expect("read committed rows");

        // Assert
        assert_eq!(
            rows_in_transaction.rows,
            vec![
                vec![Value::Int64(1), Value::String("b".to_string())],
                vec![Value::Int64(2), Value::String("a".to_string())],
                vec![Value::Int64(3), Value::String("c".to_string())],
            ]
        );
        assert!(
            duplicate_b.is_err(),
            "rotation must retain the 'b' index reservation"
        );
        assert!(
            duplicate_a.is_err(),
            "rotation must retain the 'a' index reservation"
        );
        assert_eq!(rows_after_commit.rows, rows_in_transaction.rows);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rollback_unique_reservations_when_transaction_rotation_commit_fails() {
        // Arrange
        let _failpoint_guard = document_write_failure_point_test_guard();
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_transaction_rotation_failure");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_rotation_failure (id INT PRIMARY KEY, code TEXT UNIQUE)",
            "INSERT INTO unique_rotation_failure (id, code) VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            "BEGIN",
            "UPDATE unique_rotation_failure SET code = 'tmp' WHERE id = 1",
            "UPDATE unique_rotation_failure SET code = 'a' WHERE id = 2",
            "UPDATE unique_rotation_failure SET code = 'b' WHERE id = 1",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));
        let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
        set_document_write_failure_point(None);
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("rollback failed commit");
        let duplicate = cassie.execute_sql(
            &session,
            "UPDATE unique_rotation_failure SET code = 'b' WHERE id = 3",
            vec![],
        );
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id, code FROM unique_rotation_failure ORDER BY id",
                vec![],
            )
            .expect("read rows after rollback");

        // Assert
        assert!(commit.is_err(), "the row failpoint must reject COMMIT");
        assert!(
            duplicate.is_err(),
            "rollback must preserve unique reservations"
        );
        assert_eq!(
            rows.rows,
            vec![
                vec![Value::Int64(1), Value::String("a".to_string())],
                vec![Value::Int64(2), Value::String("b".to_string())],
                vec![Value::Int64(3), Value::String("c".to_string())],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_release_unique_reservation_when_unique_column_is_dropped() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_drop_column");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_reservation_drop_column (id TEXT PRIMARY KEY, email TEXT UNIQUE, email_archive TEXT UNIQUE)",
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r1', 'reuse@example.com', 'archive@example.com')",
            "ALTER TABLE unique_reservation_drop_column DROP COLUMN email",
            "ALTER TABLE unique_reservation_drop_column ADD COLUMN email TEXT UNIQUE",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r2', 'reuse@example.com', 'new-archive@example.com')",
            vec![],
        );

        // Assert
        assert!(
            inserted.is_ok(),
            "expected the dropped-column value to be reusable"
        );
        let archived_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_drop_column (id, email, email_archive) VALUES ('r3', 'another@example.com', 'archive@example.com')",
            vec![],
        );
        assert!(
            archived_duplicate.is_err(),
            "expected the sibling UNIQUE value to remain reserved"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_release_unique_index_reservations_when_index_is_dropped() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_index_reservation_drop");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_index_reservation_drop (id TEXT PRIMARY KEY, email TEXT)",
            "INSERT INTO unique_index_reservation_drop (id, email) VALUES ('r1', 'original@example.com')",
            "CREATE UNIQUE INDEX email_idx ON unique_index_reservation_drop (email)",
            "DROP INDEX email_idx ON unique_index_reservation_drop",
            "UPDATE unique_index_reservation_drop SET email = 'changed@example.com' WHERE id = 'r1'",
            "CREATE UNIQUE INDEX email_idx ON unique_index_reservation_drop (email)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_index_reservation_drop (id, email) VALUES ('r2', 'original@example.com')",
            vec![],
        );

        // Assert
        assert!(
            inserted.is_ok(),
            "expected the dropped-index value to be reusable"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_duplicate_unique_value_after_column_rename() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_rename_column");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_reservation_rename_column (k FLOAT UNIQUE, v INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_reservation_rename_column (k, v) VALUES ($1, 1)",
                vec![Value::Float64(1.0)],
            )
            .expect("insert original row");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE unique_reservation_rename_column RENAME COLUMN k TO kk",
                vec![],
            )
            .expect("rename unique column");

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO unique_reservation_rename_column (kk, v) VALUES (1, 2)",
            vec![],
        );

        // Assert
        assert!(
            duplicate.is_err(),
            "duplicate insert unexpectedly succeeded"
        );
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT kk, v FROM unique_reservation_rename_column ORDER BY v",
                vec![],
            )
            .expect("read rows");
        assert_eq!(rows.rows, vec![vec![Value::Float64(1.0), Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_replay_unique_reservations_after_interrupted_column_rename() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("unique_reservation_rename_column_recovery");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE unique_reservation_rename_recovery (k FLOAT UNIQUE, v INT)",
            "INSERT INTO unique_reservation_rename_recovery (k, v) VALUES (1.0, 1)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        set_field_rename_failure_point(true);

        // Act
        let interrupted = cassie.execute_sql(
            &session,
            "ALTER TABLE unique_reservation_rename_recovery RENAME COLUMN k TO kk",
            vec![],
        );
        drop(cassie);
        let recovered = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        recovered.startup().expect("replay field rename");
        let session = recovered.create_session("tester", None);
        let duplicate = recovered.execute_sql(
            &session,
            "INSERT INTO unique_reservation_rename_recovery (kk, v) VALUES (1, 2)",
            vec![],
        );

        // Assert
        assert!(
            interrupted.is_err(),
            "failure point did not interrupt rename"
        );
        assert!(
            duplicate.is_err(),
            "recovered rename lost unique reservation"
        );
        let rows = recovered
            .execute_sql(
                &session,
                "SELECT kk, v FROM unique_reservation_rename_recovery ORDER BY v",
                vec![],
            )
            .expect("read rows");
        assert_eq!(rows.rows, vec![vec![Value::Float64(1.0), Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    }
}

mod foreign_key_ddl_lifecycle {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::executor::QueryResult;
    use cassie::types::Value;

    use super::support_sql as support;

    const PARENT_CHILD: [&str; 4] = [
        "CREATE TABLE p (id INT PRIMARY KEY, tag TEXT)",
        "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT cfk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE RESTRICT)",
        "INSERT INTO p (id, tag) VALUES (1, 'a')",
        "INSERT INTO c (cid, pid) VALUES (10, 1)",
    ];

    fn with_cassie(label: &str, test: impl FnOnce(&str)) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async { test(&path) });
        let _ = std::fs::remove_dir_all(&path);
    }

    fn start(path: &str) -> (Cassie, CassieSession) {
        let cassie = Cassie::new_with_data_dir(path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session)
    }

    fn run(
        cassie: &Cassie,
        session: &CassieSession,
        sql: &str,
    ) -> Result<QueryResult, CassieError> {
        cassie.execute_sql(session, sql, vec![])
    }

    fn exec_all(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
        for sql in statements {
            run(cassie, session, sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
    }

    fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        run(cassie, session, sql).expect("select").rows
    }

    fn assert_foreign_key_error(result: Result<QueryResult, CassieError>, context: &str) {
        let error = result.expect_err(context).to_string();
        assert!(
            error.contains("foreign key"),
            "{context}: unexpected error {error}"
        );
    }

    #[test]
    fn should_enforce_foreign_key_when_table_constraint_column_case_differs() {
        with_cassie("fk-ddl-column-case", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT cfk FOREIGN KEY (PID) REFERENCES p(ID) ON DELETE RESTRICT)",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (2, 1)",
                ],
            );

            // Act
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (1, 999)",
            );
            let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE id = 1");
            let key_columns = rows(
                &cassie,
                &session,
                "SELECT column_name FROM information_schema.key_column_usage WHERE constraint_name = 'cfk'",
            );

            // Assert
            assert_foreign_key_error(orphan_insert, "orphan child insert");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            assert_eq!(key_columns, vec![vec![Value::String("pid".to_string())]]);
        });
    }

    #[test]
    fn should_reject_dropping_a_table_that_a_foreign_key_still_references() {
        with_cassie("fk-ddl-drop-referenced-table", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE tree (id INT PRIMARY KEY, parent INT, CONSTRAINT tree_fk FOREIGN KEY (parent) REFERENCES tree(id))",
                    "INSERT INTO tree (id, parent) VALUES (1, NULL)",
                ],
            );

            // Act
            let referenced_drop = run(&cassie, &session, "DROP TABLE p");
            let child_insert = run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)");
            let self_referencing_drop = run(&cassie, &session, "DROP TABLE tree");
            run(&cassie, &session, "ALTER TABLE c DROP CONSTRAINT cfk").expect("drop fk");
            let released_drop = run(&cassie, &session, "DROP TABLE p");

            // Assert
            assert_foreign_key_error(referenced_drop, "drop of a referenced parent");
            assert!(child_insert.is_ok(), "the child still resolves its parent");
            assert!(
                self_referencing_drop.is_ok(),
                "a self-referencing table can be dropped"
            );
            assert!(released_drop.is_ok(), "the parent is no longer referenced");
            assert!(!cassie.catalog.exists("p"));
        });
    }

    #[test]
    fn should_enforce_foreign_key_added_by_alter_table_when_column_case_differs() {
        with_cassie("fk-ddl-alter-column-case", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT)",
                    "ALTER TABLE c ADD CONSTRAINT cfk FOREIGN KEY (PID) REFERENCES p(ID) ON DELETE RESTRICT",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (2, 1)",
                ],
            );

            // Act
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (1, 999)",
            );
            let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE id = 1");

            // Assert
            assert_foreign_key_error(orphan_insert, "orphan child insert");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
        });
    }

    #[test]
    fn should_reject_dropping_a_column_that_a_foreign_key_references() {
        with_cassie("fk-ddl-drop-referenced", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);

            // Act
            let drop_column = run(&cassie, &session, "ALTER TABLE p DROP COLUMN id");
            let restricted_delete = run(&cassie, &session, "DELETE FROM p");

            // Assert
            let error = drop_column.expect_err("referenced column drop").to_string();
            assert!(error.contains("cfk"), "unexpected error {error}");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            assert_eq!(
                rows(&cassie, &session, "SELECT id FROM p"),
                vec![vec![Value::Int64(1)]]
            );
        });
    }

    #[test]
    fn should_drop_foreign_key_with_the_child_column_it_is_declared_on() {
        with_cassie("fk-ddl-drop-child", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);

            // Act
            run(&cassie, &session, "ALTER TABLE c DROP COLUMN pid").expect("drop child column");
            drop(cassie);
            let (cassie, session) = start(path);

            // Assert
            let foreign_keys = rows(
                &cassie,
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE constraint_type = 'FOREIGN KEY'",
            );
            assert!(foreign_keys.is_empty(), "stale constraint {foreign_keys:?}");
            run(&cassie, &session, "DELETE FROM p WHERE id = 1").expect("parent is unreferenced");
        });
    }

    #[test]
    fn should_carry_foreign_key_when_the_referenced_column_is_renamed() {
        with_cassie("fk-ddl-rename-column", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);

            // Act
            run(
                &cassie,
                &session,
                "ALTER TABLE p RENAME COLUMN id TO key_id",
            )
            .expect("rename");
            drop(cassie);
            let (cassie, session) = start(path);

            // Assert
            let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE key_id = 1");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
                .expect("child of existing parent");
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (12, 999)",
            );
            assert_foreign_key_error(orphan_insert, "orphan child insert");
        });
    }

    #[test]
    fn should_carry_foreign_key_when_the_referenced_table_is_renamed() {
        with_cassie("fk-ddl-rename-table", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);

            // Act
            run(&cassie, &session, "ALTER TABLE p RENAME TO p_new").expect("rename");
            drop(cassie);
            let (cassie, session) = start(path);

            // Assert
            let restricted_delete = run(&cassie, &session, "DELETE FROM p_new WHERE id = 1");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (11, 1)")
                .expect("child of existing parent");
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (12, 999)",
            );
            assert_foreign_key_error(orphan_insert, "orphan child insert");
        });
    }

    #[test]
    fn should_carry_self_referencing_foreign_key_through_renames() {
        with_cassie("fk-ddl-self-reference", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE nodes (id INT PRIMARY KEY, parent INT)",
                    "ALTER TABLE nodes ADD CONSTRAINT nodes_parent_fk FOREIGN KEY (parent) REFERENCES nodes(id) ON DELETE RESTRICT",
                    "INSERT INTO nodes (id) VALUES (1)",
                    "INSERT INTO nodes (id, parent) VALUES (2, 1)",
                ],
            );

            // Act
            run(
                &cassie,
                &session,
                "ALTER TABLE nodes RENAME COLUMN id TO node_id",
            )
            .expect("rename column");
            run(&cassie, &session, "ALTER TABLE nodes RENAME TO tree").expect("rename table");
            drop(cassie);
            let (cassie, session) = start(path);

            // Assert
            let restricted_delete = run(&cassie, &session, "DELETE FROM tree WHERE node_id = 1");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            run(
                &cassie,
                &session,
                "INSERT INTO tree (node_id, parent) VALUES (3, 2)",
            )
            .expect("child of existing parent");
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO tree (node_id, parent) VALUES (4, 999)",
            );
            assert_foreign_key_error(orphan_insert, "orphan child insert");
            let drop_referenced = run(&cassie, &session, "ALTER TABLE tree DROP COLUMN node_id");
            assert!(drop_referenced.is_err(), "self-referenced column drop");
        });
    }

    #[test]
    fn should_resolve_referenced_relation_when_a_foreign_key_is_added_by_alter_table() {
        with_cassie("fk-ddl-alter-schema-reference", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE SCHEMA app",
                    "CREATE TABLE app.p (id INT PRIMARY KEY)",
                    "CREATE TABLE app.ch (cid INT PRIMARY KEY, pid INT)",
                    "INSERT INTO app.p (id) VALUES (1)",
                ],
            );

            // Act
            let off_path = run(
                &cassie,
                &session,
                "ALTER TABLE app.ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE",
            );
            exec_all(
                &cassie,
                &session,
                &[
                    "SET search_path = app",
                    "ALTER TABLE app.ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE",
                    "SET search_path = public",
                ],
            );
            let child_insert = run(
                &cassie,
                &session,
                "INSERT INTO app.ch (cid, pid) VALUES (10, 1)",
            );
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO app.ch (cid, pid) VALUES (11, 999)",
            );
            let cascade_delete = run(&cassie, &session, "DELETE FROM app.p WHERE id = 1");

            // Assert
            let error = off_path
                .expect_err("reference outside search_path")
                .to_string();
            assert!(error.contains("does not exist"), "unexpected error {error}");
            child_insert.expect("child of existing parent");
            assert_foreign_key_error(orphan_insert, "orphan child insert");
            cascade_delete.expect("cascade delete");
            assert!(rows(&cassie, &session, "SELECT cid FROM app.ch").is_empty());
        });
    }

    #[test]
    fn should_reject_adding_a_foreign_key_when_existing_rows_have_no_parent() {
        with_cassie("fk-ddl-existing-orphan", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE ch (cid INT PRIMARY KEY, pid INT)",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO ch (cid, pid) VALUES (9, 1)",
                    "INSERT INTO ch (cid, pid) VALUES (10, 999)",
                ],
            );

            // Act
            let add_constraint = run(
                &cassie,
                &session,
                "ALTER TABLE ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id)",
            );
            let later_orphan = run(
                &cassie,
                &session,
                "INSERT INTO ch (cid, pid) VALUES (11, 999)",
            );
            let delete_first_orphan = run(&cassie, &session, "DELETE FROM ch WHERE cid = 10");
            let delete_second_orphan = run(&cassie, &session, "DELETE FROM ch WHERE cid = 11");
            let add_after_orphans_removed = run(
                &cassie,
                &session,
                "ALTER TABLE ch ADD CONSTRAINT ch_fk FOREIGN KEY (pid) REFERENCES p(id)",
            );
            let orphan_after_successful_add = run(
                &cassie,
                &session,
                "INSERT INTO ch (cid, pid) VALUES (12, 999)",
            );
            let child_rows = rows(&cassie, &session, "SELECT cid, pid FROM ch ORDER BY cid");

            // Assert
            assert_foreign_key_error(add_constraint, "adding foreign key over an orphan row");
            later_orphan.expect("failed DDL must not persist the foreign key");
            delete_first_orphan.expect("remove first orphan");
            delete_second_orphan.expect("remove second orphan");
            add_after_orphans_removed.expect("add foreign key over valid existing rows");
            assert_foreign_key_error(orphan_after_successful_add, "orphan insert after DDL");
            assert_eq!(child_rows, vec![vec![Value::Int64(9), Value::Int64(1)]]);
        });
    }

    #[test]
    fn should_report_the_declared_foreign_key_name_on_violation() {
        with_cassie("fk-ddl-declared-name", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT my_named_fk FOREIGN KEY (pid) REFERENCES p(id))",
                    "INSERT INTO p (id) VALUES (1)",
                    "INSERT INTO c (cid, pid) VALUES (10, 1)",
                ],
            );

            // Act
            let child_error = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (1, 999)",
            )
            .expect_err("orphan child insert")
            .to_string();
            let parent_error = run(&cassie, &session, "DELETE FROM p WHERE id = 1")
                .expect_err("referenced parent delete")
                .to_string();

            // Assert
            assert!(
                child_error.contains("'my_named_fk'"),
                "child error {child_error}"
            );
            assert!(
                parent_error.contains("'my_named_fk'"),
                "parent error {parent_error}"
            );
        });
    }

    #[test]
    fn should_keep_enforcing_foreign_key_when_both_referencing_columns_are_renamed() {
        with_cassie("fk-ddl-rename-both-columns", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(&cassie, &session, &PARENT_CHILD);

            // Act
            exec_all(
                &cassie,
                &session,
                &[
                    "ALTER TABLE c RENAME COLUMN PID TO parent_id",
                    "ALTER TABLE p RENAME COLUMN ID TO key_id",
                ],
            );
            drop(cassie);
            let (cassie, session) = start(path);

            // Assert
            let restricted_delete = run(&cassie, &session, "DELETE FROM p WHERE key_id = 1");
            assert_foreign_key_error(restricted_delete, "restricted parent delete");
            run(
                &cassie,
                &session,
                "INSERT INTO c (cid, parent_id) VALUES (11, 1)",
            )
            .expect("child of existing parent");
            let orphan_insert = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, parent_id) VALUES (12, 999)",
            );
            assert_foreign_key_error(orphan_insert, "orphan child insert");
        });
    }

    #[test]
    fn should_report_the_declared_foreign_key_name_when_a_transaction_violates_it() {
        with_cassie("fk-ddl-declared-name-transaction", |path| {
            // Arrange
            let (cassie, session) = start(path);
            exec_all(
                &cassie,
                &session,
                &[
                    "CREATE TABLE p (id INT PRIMARY KEY)",
                    "CREATE TABLE c (cid INT PRIMARY KEY, pid INT, CONSTRAINT my_named_fk FOREIGN KEY (pid) REFERENCES p(id))",
                    "BEGIN",
                ],
            );

            // Act
            let error = run(
                &cassie,
                &session,
                "INSERT INTO c (cid, pid) VALUES (1, 999)",
            )
            .and_then(|_| run(&cassie, &session, "COMMIT"))
            .expect_err("orphan child in transaction")
            .to_string();

            // Assert
            assert!(error.contains("'my_named_fk'"), "transaction error {error}");
        });
    }
}

mod foreign_key_integrity {
    use cassie::app::{Cassie, CassieSession};
    use cassie::sql::ast::{CopyFormat, CopyStatement};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn start(label: &str) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie
            .execute_sql(session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
            .rows
    }

    #[test]
    fn should_refuse_to_drop_unique_index_a_foreign_key_depends_on() {
        // Arrange
        let (cassie, session, path) = start("fk-unique-index-drop");
        run(
            &cassie,
            &session,
            "CREATE TABLE p (pk INT PRIMARY KEY, id INT)",
        );
        run(&cassie, &session, "CREATE UNIQUE INDEX p_id_uniq ON p (id)");
        run(
            &cassie,
            &session,
            "CREATE TABLE c (cid INT, pid INT, CONSTRAINT cfk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE)",
        );

        // Act
        let dropped = cassie.execute_sql(&session, "DROP INDEX p_id_uniq ON p", vec![]);

        // Assert
        assert!(
            dropped.is_err(),
            "the FK's unique index must not be droppable"
        );
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO p (pk, id) VALUES (1, 100), (2, 100)",
            vec![],
        );
        assert!(duplicate.is_err(), "p.id must stay unique");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_copy_that_changes_a_referenced_parent_key() {
        // Arrange
        let (cassie, session, path) = start("fk-copy-parent-key");
        run(&cassie, &session, "CREATE TABLE p (id INT PRIMARY KEY)");
        run(
            &cassie,
            &session,
            "CREATE TABLE c (cid INT, pid INT REFERENCES p(id))",
        );
        let copy = CopyStatement {
            table: "p".to_string(),
            columns: vec!["_id".to_string(), "id".to_string()],
            format: CopyFormat::Csv,
            header: false,
        };
        cassie
            .copy_from_csv_stdin(&session, &copy, b"row-a,1\n")
            .expect("seed parent");
        run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (10, 1)");

        // Act
        let rekeyed = cassie.copy_from_csv_stdin(&session, &copy, b"row-a,2\n");

        // Assert
        assert!(
            rekeyed.is_err(),
            "COPY must not orphan c's reference to p.id = 1"
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT id FROM p"),
            vec![vec![Value::Int64(1)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_child_timestamps_equal_to_the_parent_key() {
        // Arrange
        let (cassie, session, path) = start("fk-timestamp-key");
        run(
            &cassie,
            &session,
            "CREATE TABLE pt (at TIMESTAMP PRIMARY KEY)",
        );
        run(
            &cassie,
            &session,
            "CREATE TABLE ct (id INT, at TIMESTAMP REFERENCES pt(at))",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO pt (at) VALUES ('2024-01-01 00:00:00')",
        );

        // Act
        let same_literal = cassie.execute_sql(
            &session,
            "INSERT INTO ct (id, at) VALUES (1, '2024-01-01 00:00:00')",
            vec![],
        );
        let rfc3339 = cassie.execute_sql(
            &session,
            "INSERT INTO ct (id, at) VALUES (2, '2024-01-01T00:00:00Z')",
            vec![],
        );

        // Assert
        assert!(
            same_literal.is_ok(),
            "the parent's own literal satisfies the FK"
        );
        assert!(
            rfc3339.is_ok(),
            "an equal RFC 3339 timestamp satisfies the FK"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    const NODE_DDL: &str = "CREATE TABLE node (id INT PRIMARY KEY, parent_id INT, CONSTRAINT nfk FOREIGN KEY (parent_id) REFERENCES node(id))";

    #[test]
    fn should_declare_self_referencing_foreign_key_at_create_table() {
        // Arrange
        let (cassie, session, path) = start("fk-self-reference-declare");

        // Act
        let created = cassie.execute_sql(&session, NODE_DDL, vec![]);

        // Assert
        assert!(created.is_ok(), "self-referencing FK at CREATE TABLE");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_self_pointing_row_given_self_referencing_foreign_key() {
        // Arrange
        let (cassie, session, path) = start("fk-self-reference-insert");
        run(&cassie, &session, NODE_DDL);

        // Act
        let root = cassie.execute_sql(
            &session,
            "INSERT INTO node (id, parent_id) VALUES (1, 1)",
            vec![],
        );

        // Assert
        assert!(
            root.is_ok(),
            "a self-pointing root row satisfies its own FK"
        );
        assert!(cassie
            .execute_sql(
                &session,
                "INSERT INTO node (id, parent_id) VALUES (3, 99)",
                vec![],
            )
            .is_err());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_delete_row_referenced_only_by_itself() {
        // Arrange
        let (cassie, session, path) = start("fk-self-reference-delete");
        run(&cassie, &session, NODE_DDL);
        run(
            &cassie,
            &session,
            "INSERT INTO node (id, parent_id) VALUES (1, 1)",
        );

        // Act
        let deleted = cassie.execute_sql(&session, "DELETE FROM node WHERE id = 1", vec![]);

        // Assert
        assert!(
            deleted.is_ok(),
            "a row referenced only by itself is deletable"
        );
        assert!(rows(&cassie, &session, "SELECT id FROM node").is_empty());
        let _ = std::fs::remove_dir_all(path);
    }
}

mod unique_violation_reporting {
    use cassie::app::{Cassie, CassieError, CassieSession};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn start(label: &str) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie
            .execute_sql(session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    fn violated_constraint(
        result: Result<cassie::executor::QueryResult, CassieError>,
    ) -> Option<String> {
        if let Err(CassieError::UniqueViolation { constraint, .. }) = result {
            Some(constraint)
        } else {
            None
        }
    }

    #[test]
    fn should_report_unique_index_violation_as_unique_violation() {
        // Arrange
        let (cassie, session, path) = start("unique-index-violation-sqlstate");
        run(
            &cassie,
            &session,
            "CREATE TABLE b (id INT PRIMARY KEY, email TEXT)",
        );
        run(
            &cassie,
            &session,
            "CREATE UNIQUE INDEX b_email ON b (email)",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO b (id, email) VALUES (1, 'a@x')",
        );

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO b (id, email) VALUES (2, 'a@x')",
            vec![],
        );

        // Assert
        assert_eq!(violated_constraint(duplicate).as_deref(), Some("b_email"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_declared_unique_constraint_name() {
        // Arrange
        let (cassie, session, path) = start("unique-declared-name");
        run(
            &cassie,
            &session,
            "CREATE TABLE nn (id INT PRIMARY KEY, email TEXT, CONSTRAINT my_email_key UNIQUE (email))",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO nn (id, email) VALUES (1, 'a@x')",
        );

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO nn (id, email) VALUES (2, 'a@x')",
            vec![],
        );

        // Assert
        assert_eq!(
            violated_constraint(duplicate).as_deref(),
            Some("my_email_key")
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_declared_primary_key_name() {
        // Arrange
        let (cassie, session, path) = start("primary-key-declared-name");
        run(
            &cassie,
            &session,
            "CREATE TABLE pk_named (id INT, CONSTRAINT my_pk PRIMARY KEY (id))",
        );
        run(&cassie, &session, "INSERT INTO pk_named (id) VALUES (1)");

        // Act
        let duplicate =
            cassie.execute_sql(&session, "INSERT INTO pk_named (id) VALUES (1)", vec![]);

        // Assert
        assert_eq!(violated_constraint(duplicate).as_deref(), Some("my_pk"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_unique_violation_given_conflict_outside_on_conflict_target() {
        // Arrange
        let (cassie, session, path) = start("unique-violation-outside-conflict-target");
        run(
            &cassie,
            &session,
            "CREATE TABLE oc (id INT PRIMARY KEY, email TEXT)",
        );
        run(
            &cassie,
            &session,
            "CREATE UNIQUE INDEX oc_email ON oc (email)",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO oc (id, email) VALUES (1, 'a@x')",
        );

        // Act
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO oc (id, email) VALUES (2, 'a@x') ON CONFLICT (id) DO NOTHING",
            vec![],
        );

        // Assert
        assert_eq!(violated_constraint(duplicate).as_deref(), Some("oc_email"));
        let _ = std::fs::remove_dir_all(path);
    }
}

mod on_conflict_resolution {
    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn start(label: &str) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
        assert!(
            cassie.execute_sql(session, sql, vec![]).is_ok(),
            "statement should succeed: {sql}"
        );
    }

    fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(session, sql, vec![])
            .map(|result| result.rows)
            .unwrap_or_default()
    }

    fn seed_expression_index(cassie: &Cassie, session: &CassieSession) {
        run(
            cassie,
            session,
            "CREATE TABLE a1 (id INT PRIMARY KEY, email TEXT)",
        );
        run(
            cassie,
            session,
            "CREATE UNIQUE INDEX a1_lower ON a1 ((lower(email)))",
        );
        run(
            cassie,
            session,
            "INSERT INTO a1 (id, email) VALUES (1, 'A@B.com')",
        );
    }

    #[test]
    fn should_skip_expression_index_conflict_given_untargeted_do_nothing() {
        // Arrange
        let (cassie, session, path) = start("on-conflict-expression-autocommit");
        seed_expression_index(&cassie, &session);

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO a1 (id, email) VALUES (2, 'a@b.com') ON CONFLICT DO NOTHING",
            vec![],
        );

        // Assert
        assert!(
            inserted.is_ok(),
            "ON CONFLICT DO NOTHING skips the duplicate"
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT id FROM a1"),
            vec![vec![Value::Int64(1)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_skip_expression_index_conflict_given_untargeted_do_nothing_in_transaction() {
        // Arrange
        let (cassie, session, path) = start("on-conflict-expression-transaction");
        seed_expression_index(&cassie, &session);
        run(&cassie, &session, "BEGIN");

        // Act
        run(
            &cassie,
            &session,
            "INSERT INTO a1 (id, email) VALUES (2, 'a@b.com') ON CONFLICT DO NOTHING",
        );
        let committed = cassie.execute_sql(&session, "COMMIT", vec![]);

        // Assert
        assert!(
            committed.is_ok(),
            "COMMIT succeeds with the duplicate skipped"
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT id FROM a1"),
            vec![vec![Value::Int64(1)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fold_unquoted_conflict_target_to_catalog_column() {
        // Arrange
        let (cassie, session, path) = start("on-conflict-target-case");
        run(
            &cassie,
            &session,
            "CREATE TABLE f5 (id INT PRIMARY KEY, email TEXT UNIQUE)",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO f5 (id, email) VALUES (1, 'x')",
        );

        // Act
        let skipped = cassie.execute_sql(
            &session,
            "INSERT INTO f5 (id, email) VALUES (2, 'x') ON CONFLICT (EMAIL) DO NOTHING",
            vec![],
        );
        let updated = cassie.execute_sql(
            &session,
            "INSERT INTO f5 (id, email) VALUES (3, 'x') ON CONFLICT (Email) DO UPDATE SET id = 9",
            vec![],
        );

        // Assert
        assert!(skipped.is_ok(), "ON CONFLICT (EMAIL) DO NOTHING");
        assert!(updated.is_ok(), "ON CONFLICT (Email) DO UPDATE");
        assert_eq!(
            rows(&cassie, &session, "SELECT id FROM f5"),
            vec![vec![Value::Int64(9)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_do_update_that_affects_a_row_twice() {
        // Arrange
        let (cassie, session, path) = start("on-conflict-row-twice");
        run(&cassie, &session, "CREATE TABLE t (k TEXT UNIQUE, v INT)");
        run(&cassie, &session, "INSERT INTO t (k, v) VALUES ('a', 1)");

        // Act
        let upserted = cassie.execute_sql(
            &session,
            "INSERT INTO t (k, v) VALUES ('a', 2), ('a', 3) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            vec![],
        );

        // Assert
        assert!(
            matches!(
                upserted,
                Err(cassie::app::CassieError::CardinalityViolation(_))
            ),
            "a row may not be affected twice by one ON CONFLICT DO UPDATE"
        );
        assert_eq!(
            rows(&cassie, &session, "SELECT k, v FROM t"),
            vec![vec![Value::String("a".into()), Value::Int64(1)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_do_update_that_updates_a_row_inserted_by_the_same_statement() {
        // Arrange
        let (cassie, session, path) = start("on-conflict-row-inserted-then-updated");
        run(&cassie, &session, "CREATE TABLE t (k TEXT UNIQUE, v INT)");

        // Act
        let upserted = cassie.execute_sql(
            &session,
            "INSERT INTO t (k, v) VALUES ('b', 1), ('b', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            vec![],
        );

        // Assert
        assert!(
            matches!(
                upserted,
                Err(cassie::app::CassieError::CardinalityViolation(_))
            ),
            "a row inserted by the statement may not be updated by it"
        );
        assert!(rows(&cassie, &session, "SELECT k FROM t").is_empty());
        let _ = std::fs::remove_dir_all(path);
    }
}

mod staged_unique_write_order {
    use cassie::app::{Cassie, CassieSession};

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    const ATTEMPTS: usize = 24;

    fn start(label: &str) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        (cassie, session, path)
    }

    fn commits(cassie: &Cassie, session: &CassieSession, statements: &[String]) -> bool {
        statements
            .iter()
            .all(|sql| cassie.execute_sql(session, sql, vec![]).is_ok())
    }

    #[test]
    fn should_commit_delete_then_reinsert_of_a_unique_value_in_any_row_order() {
        // Arrange
        let (cassie, session, path) = start("staged-unique-delete-reinsert");
        let _ = cassie.execute_sql(&session, "CREATE TABLE t (id INT, n INT UNIQUE)", vec![]);

        // Act
        let committed = (0..ATTEMPTS)
            .filter(|attempt| {
                let old = attempt * 2;
                let new = old + 1;
                commits(
                    &cassie,
                    &session,
                    &[
                        format!("INSERT INTO t (id, n) VALUES ({old}, 5)"),
                        "BEGIN".to_string(),
                        format!("DELETE FROM t WHERE id = {old}"),
                        format!("INSERT INTO t (id, n) VALUES ({new}, 5)"),
                        "COMMIT".to_string(),
                        format!("DELETE FROM t WHERE id = {new}"),
                    ],
                )
            })
            .count();

        // Assert
        assert_eq!(
            committed, ATTEMPTS,
            "every attempt commits deterministically"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_commit_update_then_reinsert_of_a_unique_value_in_any_row_order() {
        // Arrange
        let (cassie, session, path) = start("staged-unique-update-reinsert");
        let _ = cassie.execute_sql(&session, "CREATE TABLE t (id INT, n INT UNIQUE)", vec![]);

        // Act
        let committed = (0..ATTEMPTS)
            .filter(|attempt| {
                let old = attempt * 2;
                let new = old + 1;
                commits(
                    &cassie,
                    &session,
                    &[
                        format!("INSERT INTO t (id, n) VALUES ({old}, 5)"),
                        "BEGIN".to_string(),
                        format!("UPDATE t SET n = -{old} - 1 WHERE id = {old}"),
                        format!("INSERT INTO t (id, n) VALUES ({new}, 5)"),
                        "COMMIT".to_string(),
                        format!("DELETE FROM t WHERE id = {new}"),
                    ],
                )
            })
            .count();

        // Assert
        assert_eq!(
            committed, ATTEMPTS,
            "every attempt commits deterministically"
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

mod composite_unique_constraints {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn start(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in statements {
            run(&cassie, &session, sql);
        }
        (cassie, session, path)
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie
            .execute_sql(session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    fn violated_constraint(
        result: Result<cassie::executor::QueryResult, CassieError>,
    ) -> Option<String> {
        if let Err(CassieError::UniqueViolation { constraint, .. }) = result {
            Some(constraint)
        } else {
            None
        }
    }

    fn ids(cassie: &Cassie, session: &CassieSession, table: &str) -> Vec<Vec<Value>> {
        cassie
            .execute_sql(
                session,
                &format!("SELECT id FROM {table} ORDER BY id"),
                vec![],
            )
            .expect("select ids")
            .rows
    }

    fn int_ids(values: &[i64]) -> Vec<Vec<Value>> {
        values.iter().map(|id| vec![Value::Int64(*id)]).collect()
    }

    #[test]
    fn should_reject_only_whole_tuple_duplicates_of_a_composite_unique_constraint() {
        // Arrange
        let (cassie, session, path) = start(
            "composite-unique-tuple",
            &[
                "CREATE TABLE f1 (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT f1_ab UNIQUE (a, b))",
                "CREATE TABLE f2 (id INT PRIMARY KEY, a INT, b INT, UNIQUE (a, b))",
            ],
        );

        // Act
        let mut accepted = Vec::new();
        for table in ["f1", "f2"] {
            for sql in [
                "INSERT INTO {t} (id, a, b) VALUES (1, 10, 1)",
                "INSERT INTO {t} (id, a, b) VALUES (2, 10, 2)",
                "INSERT INTO {t} (id, a, b) VALUES (3, 11, 1)",
                "INSERT INTO {t} (id, a, b) VALUES (4, 10, NULL)",
                "INSERT INTO {t} (id, a, b) VALUES (5, 10, NULL)",
            ] {
                accepted.push(
                    cassie
                        .execute_sql(&session, &sql.replace("{t}", table), vec![])
                        .is_ok(),
                );
            }
        }
        let named_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO f1 (id, a, b) VALUES (6, 10, 1)",
            vec![],
        );
        let unnamed_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO f2 (id, a, b) VALUES (6, 11, 1)",
            vec![],
        );
        let update_duplicate =
            cassie.execute_sql(&session, "UPDATE f1 SET b = 2 WHERE id = 1", vec![]);
        let unnamed_constraint = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE table_name = 'f2' AND constraint_type = 'UNIQUE'",
                vec![],
            )
            .expect("table constraints")
            .rows;

        // Assert
        assert!(accepted.iter().all(|ok| *ok), "partial overlaps are legal");
        assert_eq!(
            violated_constraint(named_duplicate).as_deref(),
            Some("f1_ab")
        );
        assert_eq!(
            violated_constraint(unnamed_duplicate).as_deref(),
            Some("f2_a_b_key")
        );
        assert_eq!(
            violated_constraint(update_duplicate).as_deref(),
            Some("f1_ab")
        );
        assert_eq!(
            unnamed_constraint,
            vec![vec![Value::String("f2_a_b_key".to_string())]]
        );
        assert_eq!(ids(&cassie, &session, "f1"), int_ids(&[1, 2, 3, 4, 5]));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_manage_a_composite_unique_constraint_through_alter_table() {
        // Arrange
        let (cassie, session, path) = start(
            "composite-unique-alter",
            &[
                "CREATE TABLE g (id INT PRIMARY KEY, a INT, b INT)",
                "CREATE TABLE g_dup (id INT PRIMARY KEY, a INT, b INT)",
                "INSERT INTO g (id, a, b) VALUES (1, 10, 1), (2, 10, 2), (3, 11, 1)",
                "INSERT INTO g_dup (id, a, b) VALUES (1, 10, 1), (2, 10, 1)",
            ],
        );

        // Act
        let added = cassie.execute_sql(
            &session,
            "ALTER TABLE g ADD CONSTRAINT g_ab UNIQUE (a, b)",
            vec![],
        );
        let added_over_duplicate = cassie.execute_sql(
            &session,
            "ALTER TABLE g_dup ADD CONSTRAINT g_dup_ab UNIQUE (a, b)",
            vec![],
        );
        let partial_overlap = cassie.execute_sql(
            &session,
            "INSERT INTO g (id, a, b) VALUES (4, 11, 2)",
            vec![],
        );
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO g (id, a, b) VALUES (5, 10, 2)",
            vec![],
        );
        run(&cassie, &session, "ALTER TABLE g DROP CONSTRAINT g_ab");
        let after_drop = cassie.execute_sql(
            &session,
            "INSERT INTO g (id, a, b) VALUES (6, 10, 2)",
            vec![],
        );

        // Assert
        assert!(added.is_ok(), "existing rows share only one column");
        assert!(added_over_duplicate.is_err(), "existing tuple duplicate");
        assert!(partial_overlap.is_ok(), "partial overlap is legal");
        assert_eq!(violated_constraint(duplicate).as_deref(), Some("g_ab"));
        assert!(after_drop.is_ok(), "the dropped constraint is released");
        assert_eq!(ids(&cassie, &session, "g"), int_ids(&[1, 2, 3, 4, 6]));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_a_composite_unique_constraint_across_its_ddl_lifecycle() {
        // Arrange
        let (cassie, session, path) = start(
            "composite-unique-lifecycle",
            &[
                "CREATE TABLE k (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT k_ab UNIQUE (a, b))",
                "CREATE TABLE k_drop (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT k_drop_ab UNIQUE (a, b))",
                "INSERT INTO k (id, a, b) VALUES (1, 10, 1)",
                "ALTER TABLE k RENAME COLUMN a TO x",
            ],
        );

        // Act
        let renamed_duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO k (id, x, b) VALUES (2, 10, 1)",
            vec![],
        );
        let drop_backing_index = cassie.execute_sql(&session, "DROP INDEX k_ab ON k", vec![]);
        let other_column_in_second_constraint = cassie.execute_sql(
            &session,
            "ALTER TABLE k ADD CONSTRAINT k_bx UNIQUE (b, id)",
            vec![],
        );
        run(&cassie, &session, "ALTER TABLE k_drop DROP COLUMN a");
        let after_column_drop = cassie.execute_sql(
            &session,
            "INSERT INTO k_drop (id, b) VALUES (1, 1), (2, 1)",
            vec![],
        );
        let remaining_unique = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE table_name = 'k_drop' AND constraint_type = 'UNIQUE'",
                vec![],
            )
            .expect("table constraints")
            .rows;
        drop(session);
        drop(cassie);
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen");
        reopened.startup().expect("restart");
        let session = reopened.create_session("tester", None);
        let restarted_duplicate = reopened.execute_sql(
            &session,
            "INSERT INTO k (id, x, b) VALUES (3, 10, 1)",
            vec![],
        );
        let restarted_partial = reopened.execute_sql(
            &session,
            "INSERT INTO k (id, x, b) VALUES (4, 10, 2)",
            vec![],
        );

        // Assert
        assert_eq!(
            violated_constraint(renamed_duplicate).as_deref(),
            Some("k_ab")
        );
        assert!(
            drop_backing_index.is_err(),
            "the constraint needs its index"
        );
        assert!(
            other_column_in_second_constraint.is_err(),
            "b already belongs to the multi-column constraint k_ab"
        );
        assert!(
            after_column_drop.is_ok(),
            "dropping a column drops k_drop_ab"
        );
        assert!(remaining_unique.is_empty(), "k_drop_ab is gone");
        assert_eq!(
            violated_constraint(restarted_duplicate).as_deref(),
            Some("k_ab")
        );
        assert!(restarted_partial.is_ok(), "partial overlap is legal");
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_arbitrate_on_conflict_against_a_composite_unique_constraint() {
        // Arrange
        let (cassie, session, path) = start(
            "composite-unique-on-conflict",
            &[
                "CREATE TABLE h (id INT PRIMARY KEY, a INT, b INT, note TEXT, UNIQUE (a, b))",
                "INSERT INTO h (id, a, b, note) VALUES (1, 10, 1, 'first')",
            ],
        );

        // Act
        run(
            &cassie,
            &session,
            "INSERT INTO h (id, a, b, note) VALUES (2, 10, 2, 'second') ON CONFLICT DO NOTHING",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO h (id, a, b, note) VALUES (3, 10, 1, 'ignored') ON CONFLICT (a, b) DO NOTHING",
        );
        run(
            &cassie,
            &session,
            "INSERT INTO h (id, a, b, note) VALUES (4, 10, 2, 'updated') ON CONFLICT (b, a) DO UPDATE SET note = excluded.note",
        );
        let notes = cassie
            .execute_sql(&session, "SELECT id, note FROM h ORDER BY id", vec![])
            .expect("select notes")
            .rows;

        // Assert
        assert_eq!(
            notes,
            vec![
                vec![Value::Int64(1), Value::String("first".to_string())],
                vec![Value::Int64(2), Value::String("updated".to_string())],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

mod char_unique_trailing_blanks {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::types::Value;

    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn start(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
        use_local_storage();
        let path = data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in statements {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
        (cassie, session, path)
    }

    fn is_unique_violation(result: &Result<cassie::executor::QueryResult, CassieError>) -> bool {
        matches!(result, Err(CassieError::UniqueViolation { .. }))
    }

    #[test]
    fn should_treat_char_values_differing_in_trailing_blanks_as_duplicates() {
        // Arrange
        let (cassie, session, path) = start(
            "char-unique-trailing-blanks",
            &[
                "CREATE TABLE f6 (id INTEGER PRIMARY KEY, c CHAR(3) UNIQUE)",
                "CREATE TABLE f7 (id INTEGER PRIMARY KEY, c CHAR(3))",
                "CREATE UNIQUE INDEX f7_c ON f7 (c)",
                "INSERT INTO f6 (id, c) VALUES (1, 'x')",
                "INSERT INTO f7 (id, c) VALUES (1, 'x  ')",
            ],
        );

        // Act
        let constraint_duplicate =
            cassie.execute_sql(&session, "INSERT INTO f6 (id, c) VALUES (2, 'x  ')", vec![]);
        let index_duplicate =
            cassie.execute_sql(&session, "INSERT INTO f7 (id, c) VALUES (2, 'x')", vec![]);
        let padded_past_length = cassie.execute_sql(
            &session,
            "INSERT INTO f6 (id, c) VALUES (3, 'yz    ')",
            vec![],
        );
        let matched = cassie
            .execute_sql(&session, "SELECT id, c FROM f7 WHERE c = 'x'", vec![])
            .expect("select f7")
            .rows;

        // Assert
        assert!(
            is_unique_violation(&constraint_duplicate),
            "'x  ' equals 'x' in CHAR(3)"
        );
        assert!(
            is_unique_violation(&index_duplicate),
            "'x' equals 'x  ' in CHAR(3)"
        );
        assert!(
            padded_past_length.is_ok(),
            "trailing blanks beyond the length are insignificant"
        );
        assert_eq!(
            matched,
            vec![vec![Value::Int64(1), Value::String("x".to_string())]]
        );
        let _ = std::fs::remove_dir_all(path);
    }
}
