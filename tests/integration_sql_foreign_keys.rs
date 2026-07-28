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
