use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-idempotent-ddl-{name}-{}", Uuid::new_v4()))
}

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

#[test]
fn should_preserve_table_given_mismatched_if_not_exists_definition() {
    // Arrange
    with_fallback();
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
fn should_preserve_index_metadata_given_mismatched_if_not_exists_definition() {
    // Arrange
    with_fallback();
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
