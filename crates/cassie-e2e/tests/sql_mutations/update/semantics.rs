#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

#[test]
fn should_not_apply_column_defaults_during_update_or_upsert() {
    // Arrange
    let path = data_dir("update_does_not_apply_defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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
            // The conflicting proposal's default still draws a value for
            // EXCLUDED, as in PostgreSQL 18, so row 3 receives the third.
            assert_eq!(
                rows.rows(),
                vec![
                    vec![Value::Int64(1), Value::Int64(3), Value::Null],
                    vec![Value::Int64(2), Value::Int64(2), Value::Int64(1)],
                    vec![Value::Int64(3), Value::Int64(3), Value::Int64(3)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_not_apply_column_defaults_during_foreign_key_cascade_updates() {
    // Arrange
    let path = data_dir("cascade_update_does_not_apply_defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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
                rows.rows(),
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
    let path = data_dir("non_finite_write_paths");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
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
                stored.rows(),
                vec![vec![Value::Int64(1), Value::Float64(5.5)]]
            );
            assert_eq!(copied.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_maintain_include_values_after_update_delete() {
    // Arrange
    let path = data_dir("include_update_delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(updated.rows(), vec![vec![Value::String("bravo".to_string())]]);
        assert_eq!(deleted.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_update_where_returning_rows() {
    // Arrange
    let path = data_dir("update_where_returning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(updated.command(), "UPDATE 1");
        assert_eq!(updated.rows().len(), 1);
        assert!(matches!(&updated.rows()[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(updated.rows()[0][1], Value::String("alpha".to_string()));
        assert_eq!(updated.rows()[0][2], Value::String("done".to_string()));
        assert_eq!(selected.rows()[0][1], Value::String("done".to_string()));
        assert_eq!(selected.rows()[1][1], Value::String("old".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_update_returning_scalar_function() {
    // Arrange
    let path = data_dir("update_returning_function");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(updated.columns()[0].name, "normalized");
        assert_eq!(updated.rows(), vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_row_id_when_update_rewrites_row_blob() {
    // Arrange
    let path = data_dir("update_preserve_id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        let original_id = match &inserted.rows()[0][0] {
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
        assert_eq!(updated.rows()[0][0], Value::String(original_id));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_validation_failure_without_mutating_row() {
    // Arrange
    let path = data_dir("update_validation_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(selected.rows()[0][0], Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_zero_rows_for_update_without_matches() {
    // Arrange
    let path = data_dir("update_no_match");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
        assert_eq!(updated.command(), "UPDATE 0");
        assert_eq!(updated.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_with_duplicate_assignment_target() {
    // Arrange
    let path = data_dir("update_duplicate_assignment");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
    let path = data_dir("update_unknown_assignment");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
