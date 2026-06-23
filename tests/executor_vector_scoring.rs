#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
use cassie::executor;
use cassie::planner::logical::LogicalPlan;
use cassie::planner::physical::PhysicalPlan;
use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
use cassie::sql::binder;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

#[path = "support/executor.rs"]
mod support;
use support::*;

#[test]
fn should_execute_query_filters_by_vector_score_function() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
    let collection = "exec_vector_score_filter";

    let schema = Schema {
        fields: vec![FieldSchema {
            name: "embedding".to_string(),
            data_type: DataType::Vector(2),
            nullable: true,
        }],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(
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
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.0, 1.0]}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_score(embedding, '[1,0]') AS score FROM exec_vector_score_filter WHERE vector_score(embedding, '[1,0]') > 0.5",
            vec![],
        )

.expect("query should execute");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.columns[0].name, "id");
    assert_eq!(result.columns[1].name, "score");
    assert_eq!(
        result.rows[0][0],
        cassie::types::Value::String("d1".to_string())
    );
}

#[test]
fn should_execute_query_orders_by_vector_distance_function_parameterized() {
    // Arrange
    // Act
    // Assert
    with_fallback();
    let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
    let collection = "exec_vector_order_func";

    let schema = Schema {
        fields: vec![FieldSchema {
            name: "embedding".to_string(),
            data_type: DataType::Vector(2),
            nullable: true,
        }],
    };

    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(
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
            serde_json::json!({"embedding": [1.0, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d2".to_string()),
            serde_json::json!({"embedding": [0.2, 0.0]}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("d3".to_string()),
            serde_json::json!({"embedding": [10.0, 10.0]}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);
    let params = vec![cassie::types::Value::String("[1,0]".to_string())];
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM exec_vector_order_func ORDER BY vector_distance(embedding, $1) ASC",
            params,
        )
        .expect("query should execute");

    let ids = result
        .rows
        .into_iter()
        .map(|row| match &row[0] {
            cassie::types::Value::String(id) => id.clone(),
            _ => panic!("expected string id"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
    );
}

#[test]
fn should_order_by_pgvector_dot_operator() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_vector_dot_order";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
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
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_dot_order ORDER BY embedding <#> '[1,0]' ASC",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 3);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["d2".to_string(), "d1".to_string(), "d3".to_string()]
        );
    });
}

#[test]
fn should_order_by_pgvector_l2_operator() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_vector_l2_order";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
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
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [0.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_vector_l2_order ORDER BY embedding <-> '[1,0]' ASC",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 3);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["d1".to_string(), "d2".to_string(), "d3".to_string()]
        );
    });
}

#[test]
fn should_fail_query_when_vector_function_dimensions_mismatch() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_vector_mismatch";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(2),
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
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
                serde_json::json!({"embedding": [1.0, 2.0]}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT vector_distance(embedding, '[1,0,0]') FROM exec_vector_mismatch",
            vec![],
        );

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_execute_create_vector_index_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_vector_index_create_command");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        let create_index = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        let catalog_index = cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .expect("index should be in catalog");
        let stored_vector = cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .expect("vector index should be persisted");

        // Assert
        assert_eq!(create_index.command, "CREATE INDEX");
        assert_eq!(create_index.columns.len(), 0);
        assert!(matches!(catalog_index.kind, IndexKind::Vector));
        assert_eq!(catalog_index.field, "embedding");
        assert_eq!(catalog_index.fields, vec!["embedding".to_string()]);
        assert_eq!(
            catalog_index.options.get("source_field"),
            Some(&"content".to_string())
        );
        assert_eq!(catalog_index.options.get("metric"), Some(&"l2".to_string()));
        assert_eq!(stored_vector.field, "embedding");
        assert_eq!(stored_vector.source_field, "content");
        assert_eq!(stored_vector.metadata.metric, DistanceMetric::L2);
        assert_eq!(
            stored_vector.metadata.provider,
            cassie.embedding_provider.provider_name()
        );
        assert_eq!(
            stored_vector.metadata.model,
            cassie.embedding_provider.model_name().to_string()
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_hnsw_vector_index_options() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_vector_index_invalid_hnsw");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_bad_hnsw (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();

        // Act
        let err = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_bad_hnsw_embedding ON idx_vector_bad_hnsw USING vector (embedding) WITH (source_field = content, index_type = hnsw, m = 1)",
                vec![],
            )
            .expect_err("invalid hnsw options should fail");

        // Assert
        assert!(err
            .to_string()
            .contains("vector index option 'm' must be in [2, 128]"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_drop_vector_index_command() {
    // Arrange
    with_fallback();
    let path = data_dir("ddl_vector_index_drop_command");
    let cassie = Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);

        // Arrange
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE idx_vector_commands (id TEXT, content TEXT, embedding VECTOR(1536))",
                vec![],
            )

.unwrap();

        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_vector_embedding ON idx_vector_commands USING vector (embedding) WITH (source_field = content, metric = l2)",
                vec![],
            )
            .unwrap();

        // Act
        let drop_index = cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_vector_embedding ON idx_vector_commands",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(drop_index.command, "DROP INDEX");
        assert!(cassie
            .catalog
            .get_index("idx_vector_commands", "idx_vector_embedding")
            .is_none());
        assert!(cassie
            .midge
            .get_vector_index("idx_vector_commands", "embedding")

            .unwrap()
            .is_none());
    });

    let _ = std::fs::remove_dir_all(path);
}
