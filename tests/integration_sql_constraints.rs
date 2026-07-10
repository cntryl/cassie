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
fn should_insert_on_conflict_do_update() {
    // Arrange
    with_fallback();
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
    with_fallback();
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

#[test]
fn should_reject_insert_when_foreign_key_parent_is_missing() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
