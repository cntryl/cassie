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

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_maintain_include_values_after_update_delete() {
    // Arrange
    with_fallback();
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
fn should_enforce_constraints_during_ingest() {
    // Arrange
    with_fallback();
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

        let first = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 1, "email": "a@example.com", "score": 25}),
            )
            .unwrap();
        let missing_not_null = cassie
            .ingest_document("constraint_docs", serde_json::json!({"id": 2, "score": 20}));
        let duplicate = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 3, "email": "a@example.com", "score": 19}),
            );
        let rejected_check = cassie
            .ingest_document(
                "constraint_docs",
                serde_json::json!({"id": 4, "email": "b@example.com", "score": 17}),
            );

        let inserted = cassie
            .midge
            .get_document("constraint_docs", &first)

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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
fn should_reject_insert_when_unique_index_value_is_duplicate() {
    // Arrange
    with_fallback();
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
            .contains("unique index 'unique_index_email_idx' failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_unique_index_value_conflicts() {
    // Arrange
    with_fallback();
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
            .contains("unique index 'unique_index_update_email_idx' failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_check_constraint_fails() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
fn should_execute_insert_values_with_explicit_columns_returning_columns() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
        let row_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"r/insert_values_row_blob/")
            .unwrap();
        let legacy_entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"doc:insert_values_row_blob:")
            .unwrap();

        // Assert
        assert_eq!(row_entries.len(), 1);
        assert!(legacy_entries.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_missing_sparse_row_fields_as_null() {
    // Arrange
    with_fallback();
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
fn should_execute_insert_select_with_returning_rows() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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

#[test]
fn should_execute_update_where_returning_rows() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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

#[test]
fn should_execute_delete_where_returning_rows() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
fn should_delete_legacy_fallback_key_for_sql_delete() {
    // Arrange
    with_fallback();
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
            "delete_legacy_cleanup",
            &row_id,
            serde_json::json!({"title": "stale"}),
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_legacy_cleanup WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let deleted = cassie
            .midge
            .get_document("delete_legacy_cleanup", &row_id)
            .unwrap();

        // Assert
        assert!(deleted.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_is_not_null_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_is_not_null");
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
                "CREATE TABLE predicate_is_not_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_not_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_not_null WHERE archived_at IS NOT NULL",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}
