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
fn should_order_by_hybrid_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_hybrid_order";

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
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
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
                Some("zeta".to_string()),
                serde_json::json!({"title": "doc1", "body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("alpha".to_string()),
                serde_json::json!({"title": "doc2", "body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM exec_hybrid_order ORDER BY score DESC",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "score");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[1][0], Value::String("zeta".to_string()));

        let first_score = match &result.rows[0][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        let second_score = match &result.rows[1][1] {
            Value::Float64(value) => *value,
            _ => panic!("expected float score"),
        };
        assert!(first_score > second_score);
    });
}

#[test]
fn should_filter_by_hybrid_score_threshold() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_hybrid_filter";

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
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
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
                serde_json::json!({"title": "doc1", "body": "red apple", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "doc2", "body": "green apple", "embedding": [0.0, 2.0]}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_hybrid_filter WHERE hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) > 0.5",
                vec![],
            )

.expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
}

#[test]
fn should_reject_hybrid_score_with_wrong_arity() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new_with_data_dir(data_dir("cassie_new")).unwrap();
        let collection = "exec_hybrid_wrong_arity";

        let schema = Schema {
            fields: vec![FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
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

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie.execute_sql(
            &session,
            "SELECT hybrid_score(search_score(body, 'red')) FROM exec_hybrid_wrong_arity",
            vec![],
        );

        // Assert
        let error = result.expect_err("query should reject wrong arity");
        assert!(error.to_string().contains("hybrid_score"));
    });
}
