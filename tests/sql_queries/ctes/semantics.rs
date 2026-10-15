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
fn should_execute_sql_with_non_recursive_cte() {
    // Arrange
    use_local_storage();
    let path = data_dir("cte_non_recursive");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_cte";

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

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "hello"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "world"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH docs_cte AS (SELECT title FROM integration_cte WHERE title = 'alpha') SELECT title FROM docs_cte",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(result.rows()[0][0], cassie::types::Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_sql_with_recursive_cte() {
    // Arrange
    use_local_storage();
    let path = data_dir("cte_recursive");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_recursive_cte";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "n".to_string(),
                data_type: DataType::Int,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(collection, Some("d1".to_string()), serde_json::json!({"n": 1}))

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM counter WHERE n < 2) SELECT n FROM counter ORDER BY n",
            vec![],
            )

.unwrap();

        let rows = result
            .rows()
            .iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                _ => panic!("expected integer value"),
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(rows, vec![1, 2]);
        let _ = std::fs::remove_dir_all(path);
    });
}
#[test]
fn should_report_declared_column_types_when_selecting_from_a_cte() {
    // Arrange
    use_local_storage();
    let path = data_dir("cte_result_column_metadata");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE cte_typed_rows (label TEXT, score INT, ratio FLOAT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cte_typed_rows (label, score, ratio) VALUES ('a', 7, 1.5)",
            vec![],
        )
        .expect("insert row");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH source AS (SELECT label, score, ratio FROM cte_typed_rows) \
                 SELECT label, score, ratio FROM source",
            vec![],
        )
        .expect("select from CTE");

    // Assert
    // A CTE's output columns keep the types of the query that defines it.
    // Reporting every column as text makes a strictly typed client decode
    // an integer as a string.
    let types = result
        .columns()
        .iter()
        .map(|column| (column.name.as_str(), column.data_type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        types,
        vec![
            ("label", "text"),
            ("score", DataType::Int.type_name().as_str()),
            ("ratio", DataType::Float.type_name().as_str()),
        ]
    );
    assert_eq!(result.columns()[1].type_oid, DataType::Int.type_oid());
    assert_eq!(result.columns()[2].type_oid, DataType::Float.type_oid());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_describe_cte_column_types_before_execution() {
    // Arrange
    // The extended protocol describes a statement without running it, so
    // this path needs the CTE's types just as the result path does.
    use_local_storage();
    let path = data_dir("cte_describe_column_metadata");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE cte_described_rows (label TEXT, score INT)",
            vec![],
        )
        .expect("create table");

    // Act
    let columns = cassie
        .describe_sql(
            "WITH source AS (SELECT label, score FROM cte_described_rows) \
                 SELECT label, score FROM source",
        )
        .expect("describe CTE select");

    // Assert
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].data_type, "text");
    assert_eq!(columns[1].type_oid, DataType::Int.type_oid());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_typed_aggregate_aliases_through_a_cte() {
    // Arrange
    use_local_storage();
    let path = data_dir("cte_aggregate_column_metadata");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE cte_aggregated_rows (label TEXT, score INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO cte_aggregated_rows (label, score) VALUES ('a', 7)",
            vec![],
        )
        .expect("insert row");

    // Act
    // The CTE renames its output columns, so the alias list has to carry
    // the underlying types too.
    let result = cassie
        .execute_sql(
            &session,
            "WITH totals (name, seen) AS \
                 (SELECT label, COUNT(score) FROM cte_aggregated_rows GROUP BY label) \
                 SELECT name, seen FROM totals",
            vec![],
        )
        .expect("select aggregate from CTE");

    // Assert
    assert_eq!(result.columns()[0].name, "name");
    assert_eq!(result.columns()[0].data_type, "text");
    assert_eq!(result.columns()[1].name, "seen");
    assert_eq!(result.columns()[1].type_oid, DataType::BigInt.type_oid());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_column_types_through_a_recursive_cte() {
    // Arrange
    use_local_storage();
    let path = data_dir("cte_recursive_column_metadata");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "WITH RECURSIVE counted (step) AS \
                 (SELECT 1 UNION ALL SELECT step + 1 FROM counted WHERE step < 3) \
                 SELECT step FROM counted",
            vec![],
        )
        .expect("select from recursive CTE");

    // Assert
    assert_eq!(result.columns()[0].name, "step");
    assert_eq!(
        result.columns()[0].type_oid,
        DataType::Int.type_oid(),
        "a recursive CTE's anchor term types its output"
    );
    let _ = std::fs::remove_dir_all(path);
}
