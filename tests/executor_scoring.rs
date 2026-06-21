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

#[tokio::test]
async fn execute_query_filters_by_vector_score_function() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
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

#[tokio::test]
async fn execute_query_orders_by_vector_distance_function_parameterized() {
    with_fallback();
    let cassie = Cassie::new().unwrap();
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
fn should_apply_fulltext_index_params_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_k1_b";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "bravo"}),
            )

            .unwrap();

        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_k1_b ON exec_fulltext_k1_b USING fulltext (body) WITH (k1 = 0, b = 0)",
                vec![],
            )

.unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_k1_b WHERE id = 'd1'",
                vec![],
            )
            .expect("query should execute");

        // Assert
        let expected = cassie::search::bm25::bm25_score(3.0, 1.0, 2.0, 0.0, 0.0, 3.0, 2.0);
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "score");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::Float64(score) => assert_eq!(*score, expected),
            _ => panic!("expected float score"),
        }
    });
}

#[test]
fn should_apply_fulltext_analyzer_stop_words_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_fulltext_analyzer_stop_words (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_fulltext_analyzer_stop_words (id, body) VALUES ('d1', 'the the alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_analyzer_stop_words ON exec_fulltext_analyzer_stop_words USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'the') AS score FROM exec_fulltext_analyzer_stop_words",
                vec![],
            )
            .expect("query should execute");

        // Assert
        match &result.rows[0][0] {
            Value::Float64(score) => assert!(*score > 0.0),
            _ => panic!("expected float score"),
        }
    });
}

#[test]
fn should_reject_unknown_fulltext_analyzer() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_fulltext_bad_analyzer (body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let err = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_bad_analyzer ON exec_fulltext_bad_analyzer USING fulltext (body) WITH (analyzer = unsupported)",
                vec![],
            )
            .expect_err("unknown analyzer should fail");

        // Assert
        assert!(err
            .to_string()
            .contains("unsupported fulltext analyzer 'unsupported'"));
    });
}

#[test]
fn should_reject_unknown_fulltext_tokenizer() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_fulltext_bad_tokenizer (body TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let err = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_exec_fulltext_bad_tokenizer ON exec_fulltext_bad_tokenizer USING fulltext (body) WITH (tokenizer = unsupported)",
                vec![],
            )
            .expect_err("unknown tokenizer should fail");

        // Assert
        assert!(err
            .to_string()
            .contains("unsupported tokenizer 'unsupported'"));
    });
}

#[test]
fn should_reject_non_finite_fulltext_index_options_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_non_finite";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();

        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_non_finite".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1e999".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })

            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_non_finite WHERE id = 'd1'",
                vec![],
            )
            ;

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_reject_duplicate_fulltext_indexes_during_search_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_fulltext_duplicate";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();

        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_duplicate_a".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1.2".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })

            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_fulltext_duplicate_b".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "2.0".to_string()),
                    ("k1".to_string(), "0.5".to_string()),
                    ("b".to_string(), "0.4".to_string()),
                ]),
            })

            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search_score(body, 'alpha') AS score FROM exec_fulltext_duplicate WHERE id = 'd1'",
                vec![],
            )
            ;

        // Assert
        assert!(result.is_err());
    });
}

#[test]
fn should_allow_plain_select_with_non_finite_fulltext_metadata() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_plain_select_bad_fulltext";

        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_exec_plain_select_bad_fulltext".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "inf".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            })
            .unwrap();

        cassie.hydrate_catalog().unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_plain_select_bad_fulltext WHERE id = 'd1'",
                vec![],
            )
            .expect("plain select should execute");

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
    });
}

#[test]
fn should_project_snippet_function_output_for_text_matches() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_snippet_output";

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
                serde_json::json!({"title": "alpha", "body": "Rust enables fast query search"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'query') AS excerpt FROM exec_snippet_output WHERE title = 'alpha'",
                vec![],
            )

.expect("snippet query should execute");

        // Assert
        assert_eq!(result.columns[0].name, "excerpt");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 1);
        match &result.rows[0][0] {
            Value::String(excerpt) => {
                assert_eq!(excerpt, "Rust enables fast <mark>query</mark> search");
            }
            _ => panic!("expected string snippet output"),
        }
    });
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
        let cassie = Cassie::new().unwrap();
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
        let cassie = Cassie::new().unwrap();
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
        let cassie = Cassie::new().unwrap();
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
fn should_order_by_hybrid_score() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
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
        let cassie = Cassie::new().unwrap();
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
        let cassie = Cassie::new().unwrap();
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
