use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

fn start_vector_cassie(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    cassie
}

fn create_sourced_vector_table(cassie: &Cassie, table: &str) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (id TEXT, body TEXT, embedding VECTOR(3))"),
            vec![],
        )
        .expect("create vector table");
    cassie
            .execute_sql(
                &session,
                &format!(
                    "CREATE INDEX {table}_idx ON {table} USING vector (embedding) WITH (source_field = body)"
                ),
                vec![],
            )
            .expect("create sourced vector index");
}

fn embedding_is_null(cassie: &Cassie, table: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("SELECT id, embedding IS NULL FROM {table} ORDER BY id"),
            vec![],
        )
        .expect("read embeddings")
        .rows()
        .to_vec()
}

#[test]
fn should_drop_vector_index_given_its_source_column_is_dropped() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_source_column_drop");
    let table = "vector_source_column_drop";
    {
        let cassie = start_vector_cassie(&path);
        create_sourced_vector_table(&cassie, table);
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                &format!("ALTER TABLE {table} DROP COLUMN body"),
                vec![],
            )
            .expect("drop source column");

        // Assert
        assert!(cassie
            .catalog
            .get_vector_index(table, "embedding")
            .is_none());
        assert!(cassie
            .catalog
            .get_index(table, &format!("{table}_idx"))
            .is_none());
    }
    let restarted = start_vector_cassie(&path);
    assert!(
        restarted
            .catalog
            .get_vector_index(table, "embedding")
            .is_none(),
        "the stored vector index record must be dropped with its source column"
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_unregister_vector_index_given_its_vector_column_is_dropped() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_column_drop_catalog");
    let table = "vector_column_drop_catalog";
    let cassie = start_vector_cassie(&path);
    create_sourced_vector_table(&cassie, table);
    let session = cassie.create_session("tester", None);

    // Act
    cassie
        .execute_sql(
            &session,
            &format!("ALTER TABLE {table} DROP COLUMN embedding"),
            vec![],
        )
        .expect("drop vector column");

    // Assert
    assert!(cassie
        .catalog
        .get_vector_index(table, "embedding")
        .is_none());
    assert!(cassie
        .catalog
        .get_index(table, &format!("{table}_idx"))
        .is_none());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_write_null_embedding_given_source_column_is_omitted_or_null() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_source_column_omitted");
    let table = "vector_source_column_omitted";
    let cassie = start_vector_cassie(&path);
    create_sourced_vector_table(&cassie, table);
    let session = cassie.create_session("tester", None);
    let copy = CopyStatement {
        table: table.to_string(),
        columns: vec!["id".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let omitted_insert = cassie.execute_sql(
        &session,
        &format!("INSERT INTO {table} (id) VALUES ('a-omitted')"),
        vec![],
    );
    let explicit_null = cassie.execute_sql(
        &session,
        &format!("INSERT INTO {table} (id, body) VALUES ('b-null', NULL)"),
        vec![],
    );
    let omitted_copy = cassie.copy_from_csv_stdin(&session, &copy, b"c-copy\n");

    // Assert
    assert!(omitted_insert.is_ok(), "INSERT omitting the source column");
    assert!(explicit_null.is_ok(), "INSERT with a NULL source column");
    assert!(omitted_copy.is_ok(), "COPY omitting the source column");
    assert_eq!(
        embedding_is_null(&cassie, table),
        vec![
            vec![Value::String("a-omitted".into()), Value::Bool(true)],
            vec![Value::String("b-null".into()), Value::Bool(true)],
            vec![Value::String("c-copy".into()), Value::Bool(true)],
        ]
    );
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
