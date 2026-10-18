#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, canonical_schema_name};
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
fn should_execute_sql_query_after_catalog_hydration() {
    // Arrange
    use_local_storage();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = canonical_relation_name("postgres", "public", "sql_hydration");
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie.midge.create_collection(&collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                &collection,
                None,
                serde_json::json!({"title": "sql", "body": "hybrid path"}),
            )
            .unwrap();

        // Act
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM sql_hydration WHERE title = 'sql'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.columns()[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_namespace_on_create_schema() {
    // Arrange
    use_local_storage();
    let path = data_dir("create_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie
            .midge
            .ensure_families_ready()
            .expect("families ready");

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command(), "CREATE SCHEMA");
        assert!(cassie.catalog.namespace_exists("analytics"));
        assert!(cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_schema_through_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("rename_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let next_schema = canonical_schema_name("postgres", "reporting_archive");
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command(), "ALTER SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting"));
        assert!(cassie.catalog.namespace_exists("reporting_archive"));
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));
        assert!(cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == &next_schema));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_drop_schema_through_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("drop_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(&session, "DROP SCHEMA reporting", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command(), "DROP SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting"));
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_duplicate_create_schema_when_if_not_exists_is_set() {
    // Arrange
    use_local_storage();
    let path = data_dir("create_schema_if_not_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.create_namespace("analytics").unwrap();

        let initial = cassie.midge.list_namespaces();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA IF NOT EXISTS analytics", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command(), "CREATE SCHEMA");
        let namespaced = cassie.midge.list_namespaces();
        assert_eq!(namespaced.len(), initial.len());
        assert!(namespaced.iter().any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_column_through_sql() {
    // Arrange
    use_local_storage();
    let path = data_dir("rename_column");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let relation = canonical_relation_name("postgres", "public", "rename_column_docs");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                &relation,
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT id, headline FROM rename_column_docs ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command(), "ALTER TABLE");
        assert_eq!(selected.rows().len(), 1);
        assert_eq!(selected.rows()[0][0], Value::String("d1".to_string()));
        assert_eq!(selected.rows()[0][1], Value::String("alpha".to_string()));
        let schema = cassie
            .catalog
            .get_schema(&relation)
            .expect("schema should exist");
        assert!(schema.fields.iter().any(|field| field.name == "headline"));
        assert!(!schema.fields.iter().any(|field| field.name == "title"));

        let _ = std::fs::remove_dir_all(path);
    });
}

fn run_case_insensitive_catalog_scenario(
    label: &str,
    setup: &[&str],
    act: &str,
    probe: &str,
) -> (Result<(), String>, Result<usize, String>) {
    use_local_storage();
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let outcome = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        let acted = cassie
            .execute_sql(&session, act, vec![])
            .map(|_| ())
            .map_err(|error| error.to_string());
        let probed = cassie
            .execute_sql(&session, probe, vec![])
            .map(|result| result.rows().len())
            .map_err(|error| error.to_string());
        (acted, probed)
    });
    let _ = std::fs::remove_dir_all(path);
    outcome
}

#[test]
fn should_drop_table_when_reference_case_differs_from_stored_name() {
    // Arrange
    let setup = ["CREATE TABLE CaseDropDocs (title TEXT)"];

    // Act
    let (dropped, probed) = run_case_insensitive_catalog_scenario(
        "case_drop_table",
        &setup,
        "DROP TABLE casedropdocs",
        "SELECT title FROM CaseDropDocs",
    );

    // Assert
    assert_eq!(dropped, Ok(()));
    assert!(probed.is_err(), "table still readable: {probed:?}");
}

#[test]
fn should_rename_table_when_reference_case_differs_from_stored_name() {
    // Arrange
    let setup = [
        "CREATE TABLE CaseRenameDocs (title TEXT)",
        "INSERT INTO CaseRenameDocs (title) VALUES ('alpha')",
    ];

    // Act
    let (renamed, probed) = run_case_insensitive_catalog_scenario(
        "case_rename_table",
        &setup,
        "ALTER TABLE caserenamedocs RENAME TO case_renamed_docs",
        "SELECT title FROM case_renamed_docs",
    );

    // Assert
    assert_eq!(renamed, Ok(()));
    assert_eq!(probed, Ok(1));
}

#[test]
fn should_drop_index_when_reference_case_differs_from_stored_name() {
    // Arrange
    let setup = [
        "CREATE TABLE CaseIndexDocs (title TEXT)",
        "CREATE INDEX CaseTitleIdx ON CaseIndexDocs (title)",
    ];

    // Act
    let (dropped, probed) = run_case_insensitive_catalog_scenario(
        "case_drop_index",
        &setup,
        "DROP INDEX casetitleidx ON caseindexdocs",
        "SELECT indexname FROM pg_catalog.pg_indexes WHERE tablename = 'CaseIndexDocs'",
    );

    // Assert
    assert_eq!(dropped, Ok(()));
    assert_eq!(probed, Ok(0));
}

#[test]
fn should_rename_schema_when_reference_case_differs_from_stored_name() {
    // Arrange
    let setup = [
        "CREATE SCHEMA CaseReporting",
        "CREATE TABLE CaseReporting.events (title TEXT)",
    ];

    // Act
    let (renamed, probed) = run_case_insensitive_catalog_scenario(
        "case_rename_schema",
        &setup,
        "ALTER SCHEMA casereporting RENAME TO case_archive",
        "SELECT title FROM case_archive.events",
    );

    // Assert
    assert_eq!(renamed, Ok(()));
    assert_eq!(probed, Ok(0));
}

#[test]
fn should_drop_schema_when_reference_case_differs_from_stored_name() {
    // Arrange
    let setup = ["CREATE SCHEMA CaseScratch"];

    // Act
    let (dropped, probed) = run_case_insensitive_catalog_scenario(
        "case_drop_schema",
        &setup,
        "DROP SCHEMA casescratch",
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name = 'CaseScratch'",
    );

    // Assert
    assert_eq!(dropped, Ok(()));
    assert_eq!(probed, Ok(0));
}
