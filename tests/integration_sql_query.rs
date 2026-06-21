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
fn should_execute_sql_query_after_catalog_hydration() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "sql_hydration";
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

        cassie.midge.create_collection(collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
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
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_column_top_k_with_deterministic_tie_break() {
    // Arrange
    with_fallback();
    let path = data_dir("column_top_k_tie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_column_top_k_tie";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
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
                Some("d2".to_string()),
                serde_json::json!({"title": "second", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "first", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "third", "score": 1}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM sql_column_top_k_tie ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_filtered_ordered_column_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("column_top_k_filter_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_column_top_k_filter_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
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
                serde_json::json!({"title": "skip", "score": 100}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "keep", "score": 10}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM sql_column_top_k_filter_fallback WHERE title = 'keep' ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_function_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_function_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_function_fallback";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
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
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT upper(title) FROM sql_projected_scan_function_fallback WHERE title = 'alpha'",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("ALPHA".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_text_scalar_functions_query() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_text_functions");
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
                "CREATE TABLE scalar_text_functions (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_text_functions (title) VALUES ('  Alpha  ')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT lower(title) AS lowered, upper(title) AS raised, length(title) AS chars, substring(title, 3, 5) AS slice, trim(title) AS trimmed, concat(trim(title), '-done') AS combined FROM scalar_text_functions",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("  alpha  ".to_string()),
                Value::String("  ALPHA  ".to_string()),
                Value::Int64(9),
                Value::String("Alpha".to_string()),
                Value::String("Alpha".to_string()),
                Value::String("Alpha-done".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_coalesce_scalar_function_query() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_coalesce_function");
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
                "CREATE TABLE scalar_coalesce_function (title TEXT, fallback TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_coalesce_function (title, fallback) VALUES (NULL, 'backup')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT coalesce(title, fallback, 'missing') AS value FROM scalar_coalesce_function",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("backup".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_numeric_scalar_function_query() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_numeric_function");
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
                "CREATE TABLE scalar_numeric_function (delta INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO scalar_numeric_function (delta) VALUES (-42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT abs(delta) AS magnitude FROM scalar_numeric_function",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_wildcard_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_wildcard_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_wildcard_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
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
                serde_json::json!({"title": "alpha", "score": 7}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM sql_projected_scan_wildcard_fallback WHERE score = 7",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][2], Value::Int64(7));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_cosine_distance_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("cosine_distance_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_cosine_distance_projection";
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
                Some("same".to_string()),
                serde_json::json!({"embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("orthogonal".to_string()),
                serde_json::json!({"embedding": [0.0, 1.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, cosine_distance(embedding, '[1,0]') AS distance FROM sql_cosine_distance_projection ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("orthogonal".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));
        assert_eq!(result.rows[1][0], Value::String("same".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_dot_product_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("dot_product_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_dot_product_projection";
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT dot_product(embedding, '[3,4]') AS score FROM sql_dot_product_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(11.0)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_l2_distance_for_vector_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("l2_distance_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_l2_distance_projection";
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
                serde_json::json!({"embedding": [4.0, 6.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT vector_distance(embedding, '[1,2]') AS distance FROM sql_l2_distance_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Float64(5.0)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_pgvector_operator_distances() {
    // Arrange
    with_fallback();
    let path = data_dir("pgvector_operator_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_pgvector_operator_projection";
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
                serde_json::json!({"embedding": [2.0, 0.0]}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT embedding <-> '[1,0]' AS l2, embedding <=> '[1,0]' AS cosine, embedding <#> '[1,0]' AS dot FROM sql_pgvector_operator_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Float64(1.0),
                Value::Float64(0.0),
                Value::Float64(-2.0)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_fulltext_top_k_by_score_with_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_limit";
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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha alpha alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "beta gamma"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_limit WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_unordered_fulltext_query_with_matching_search_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_match");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_match";
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
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "first", "body": "alpha beta"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "second", "body": "bravo"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "third", "body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_match WHERE search(body, 'alpha') LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::String("third".to_string()));
        assert!(matches!(result.rows[0][2], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_search_function_as_boolean_match() {
    // Arrange
    with_fallback();
    let path = data_dir("search_boolean_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_boolean_projection";
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
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT search(body, 'alpha') AS matches_alpha, search(body, 'gamma') AS matches_gamma FROM sql_search_boolean_projection",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Bool(true), Value::Bool(false)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_search_score_as_numeric_relevance() {
    // Arrange
    with_fallback();
    let path = data_dir("search_score_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_search_score_projection";
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
                serde_json::json!({"body": "alpha alpha beta"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "gamma delta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_search_score_projection ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(score) if score > 0.0));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[1][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unordered_fulltext_mismatched_search_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_mismatch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_mismatch";
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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha"}),
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

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_mismatch WHERE search(body, 'bravo')",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unordered_fulltext_additional_filters_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_unordered_extra_filter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_unordered_extra_filter";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
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
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_unordered_extra_filter WHERE search(body, 'alpha') AND status = 'approved'",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_hybrid_top_k_by_score_with_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_top_k_limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_limit";
        let schema = Schema {
            fields: vec![
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
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "red", "embedding": [10.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "red", "embedding": [1.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_limit ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value > 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_generate_hybrid_candidates_from_text_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_text_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_text_candidates";
        let schema = Schema {
            fields: vec![
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
                Some("text_match".to_string()),
                serde_json::json!({"body": "red", "embedding": [100.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("vector_only".to_string()),
                serde_json::json!({"body": "blue", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        let before = cassie.metrics();
        let before_candidates = before["hybrid"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_text_candidates ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("text_match".to_string()));
        assert_eq!(
            after["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_hybrid_text_candidate_without_vector() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_missing_vector");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_missing_vector";
        let schema = Schema {
            fields: vec![
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
                Some("text_without_vector".to_string()),
                serde_json::json!({"body": "red"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("ignored_non_match".to_string()),
                serde_json::json!({"body": "blue"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_missing_vector ORDER BY score DESC LIMIT 1",
                vec![],
            );

        // Assert
        let error = result.expect_err("text candidate should require a vector");
        assert!(error.to_string().contains("vector_score expects vector"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_complex_fulltext_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_complex_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_complex_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
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
            );        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha alpha alpha", "status": "pending"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"body": "alpha", "status": "approved"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_complex_fallback WHERE search(body, 'alpha') AND status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_snippet_without_highlighting_generated_markup() {
    // Arrange
    with_fallback();
    let path = data_dir("snippet_generated_markup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_snippet_generated_markup";
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
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"body": "alpha beta"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT snippet(body, 'alpha mark') AS excerpt FROM sql_snippet_generated_markup",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("<mark>alpha</mark> beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_describe_select_projection_with_column_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("describe_sql_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_describe_metadata";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
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

        // Act
        let columns = cassie
            .describe_sql("SELECT id, title, score FROM sql_describe_metadata")
            .unwrap();

        // Assert
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].name, "id");
        assert_eq!(columns[0].type_oid, DataType::Text.type_oid());
        assert_eq!(columns[1].name, "title");
        assert_eq!(columns[1].data_type, "text");
        assert_eq!(columns[2].name, "score");
        assert_eq!(columns[2].type_oid, DataType::Int.type_oid());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_select_query_plan() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_select_plan");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_explain_select_plan";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_select_plan WHERE title = 'alpha' ORDER BY title LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.columns[0].name, "QUERY PLAN");
        assert_eq!(result.rows.len(), 1);
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("collection=sql_explain_select_plan"));
        assert!(plan.contains("operators=Scan>Filter>Sort>Project>Offset>Limit"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_predicate_pushdown_for_literal_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_predicate_pushdown");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_explain_predicate_pushdown";
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
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_predicate_pushdown WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("predicate_pushdown=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_projection_pruning_fields() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_projection_pruning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_explain_projection_pruning";
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
                    name: "summary".to_string(),
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_projection_pruning WHERE body = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("projection_pruning=true"));
        assert!(plan.contains("scan_fields=title,body"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_limit_pushdown_scan_limit() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_limit_pushdown");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_explain_limit_pushdown";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
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
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_limit_pushdown LIMIT 20 OFFSET 5",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("limit_pushdown=true"));
        assert!(plan.contains("scan_limit=25"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_fulltext_analyzer_options_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_analyzer_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_fulltext_analyzer_restart (id TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_fulltext_analyzer_restart (id, body) VALUES ('d1', 'the alpha marker')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_sql_fulltext_analyzer_restart ON sql_fulltext_analyzer_restart USING fulltext (body) WITH (analyzer = standard, stop_words = none)",
                    vec![],
                )
                .unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let index = restarted
            .catalog
            .get_index(
                "sql_fulltext_analyzer_restart",
                "idx_sql_fulltext_analyzer_restart",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT search(body, 'the') AS matched, snippet(body, 'the') AS excerpt FROM sql_fulltext_analyzer_restart",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(
            index.options.get("stop_words"),
            Some(&"none".to_string())
        );
        assert_eq!(result.rows[0][0], Value::Bool(true));
        let Value::String(excerpt) = &result.rows[0][1] else {
            panic!("expected snippet string");
        };
        assert!(excerpt.contains("<mark>the</mark>"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_fulltext_tokenizer_options_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_tokenizer_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_fulltext_tokenizer_restart (body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_fulltext_tokenizer_restart (body) VALUES ('alpha-beta gamma')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_sql_fulltext_tokenizer_restart ON sql_fulltext_tokenizer_restart USING fulltext (body) WITH (tokenizer = whitespace, stop_words = none)",
                    vec![],
                )
                .unwrap();
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let index = restarted
            .catalog
            .get_index(
                "sql_fulltext_tokenizer_restart",
                "idx_sql_fulltext_tokenizer_restart",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT search(body, 'alpha') AS standard_match, search(body, 'alpha-beta') AS whitespace_match, snippet(body, 'alpha-beta') AS excerpt FROM sql_fulltext_tokenizer_restart",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(
            index.options.get("tokenizer"),
            Some(&"whitespace".to_string())
        );
        assert_eq!(result.rows[0][0], Value::Bool(false));
        assert_eq!(result.rows[0][1], Value::Bool(true));
        let Value::String(excerpt) = &result.rows[0][2] else {
            panic!("expected snippet string");
        };
        assert!(excerpt.contains("<mark>alpha-beta</mark>"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_top_k_plan_for_order_limit_query() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_top_k");
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
                "CREATE TABLE sql_explain_top_k (title TEXT, score INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_top_k ORDER BY score DESC LIMIT 5",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("top_k=true"));
        assert!(plan.contains("top_k_limit=5"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_analyze_select_query_plan() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_analyze_select");
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
                "CREATE TABLE sql_explain_analyze_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_explain_analyze_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN ANALYZE SELECT title FROM sql_explain_analyze_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("analyze=true"));
        assert!(plan.contains("actual_rows=1"));
        assert!(plan.contains("diagnostics="));
        assert!(plan.contains("storage_reads_delta:"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_namespace_on_create_schema() {
    // Arrange
    with_fallback();
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
        assert_eq!(result.command, "CREATE SCHEMA");
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
    with_fallback();
    let path = data_dir("rename_schema");
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
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "ALTER SCHEMA");
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
            .any(|name| name == "reporting_archive"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_drop_schema_through_sql() {
    // Arrange
    with_fallback();
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
        assert_eq!(result.command, "DROP SCHEMA");
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
    with_fallback();
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
        assert_eq!(result.command, "CREATE SCHEMA");
        let namespaced = cassie.midge.list_namespaces();
        assert_eq!(namespaced.len(), initial.len());
        assert!(namespaced.iter().any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_column_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_column");
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
                "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "rename_column_docs",
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
        assert_eq!(result.command, "ALTER TABLE");
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(selected.rows[0][1], Value::String("alpha".to_string()));
        let schema = cassie
            .catalog
            .get_schema("rename_column_docs")
            .expect("schema should exist");
        assert!(schema.fields.iter().any(|field| field.name == "headline"));
        assert!(!schema.fields.iter().any(|field| field.name == "title"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_is_null_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_is_null");
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
                "CREATE TABLE predicate_is_null (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('alpha', NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_is_null (title, archived_at) VALUES ('beta', 'today')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_is_null WHERE archived_at IS NULL",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_in_list_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_in_list");
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
                "CREATE TABLE predicate_in_list (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_in_list WHERE title IN ('alpha', 'beta')",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_in_list_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not_in_list");
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
                "CREATE TABLE predicate_not_in_list (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_in_list (title) VALUES ('gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_in_list WHERE title NOT IN ('alpha', 'beta')",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("gamma".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_between_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_between");
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
                "CREATE TABLE predicate_between (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_between WHERE score BETWEEN 10 AND 20",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_between_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not_between");
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
                "CREATE TABLE predicate_not_between (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('alpha', 5)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_between (title, score) VALUES ('beta', 15)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_between WHERE score NOT BETWEEN 10 AND 20",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_cast_function_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_cast_function");
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
                "CREATE TABLE predicate_cast_function (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_cast_function (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_cast_function WHERE CAST(score AS TEXT) = '10'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_postgres_style_cast_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_pg_cast");
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
                "CREATE TABLE predicate_pg_cast (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_pg_cast (title, score) VALUES ('alpha', 10)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_pg_cast WHERE score::TEXT = '10'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_project_rows_with_cast_expressions() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_cast_expressions");
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
                "CREATE TABLE projection_cast_expressions (score INT, active BOOLEAN, flag TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_cast_expressions (score, active, flag) VALUES (10, true, 't')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT CAST(score AS TEXT) AS score_text, score::FLOAT AS score_float, CAST(active AS INT) AS active_int, CAST(flag AS BOOLEAN) AS flag_bool FROM projection_cast_expressions",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("10".to_string()),
                Value::Float64(10.0),
                Value::Int64(1),
                Value::Bool(true)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_invalid_cast_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("invalid_cast_expression");
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
                "CREATE TABLE invalid_cast_expression (label TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO invalid_cast_expression (label) VALUES ('not-a-number')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie.execute_sql(
            &session,
            "SELECT CAST(label AS INT) FROM invalid_cast_expression",
            vec![],
        );

        // Assert
        assert!(selected.is_err());
        assert!(selected
            .unwrap_err()
            .to_string()
            .contains("cannot cast value to INT"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_nulls_first_when_requested() {
    // Arrange
    with_fallback();
    let path = data_dir("order_nulls_first");
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
                "CREATE TABLE order_nulls_first (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_first (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_first ORDER BY archived_at NULLS FIRST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("beta".to_string())],
                vec![Value::String("alpha".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_order_nulls_last_when_requested() {
    // Arrange
    with_fallback();
    let path = data_dir("order_nulls_last");
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
                "CREATE TABLE order_nulls_last (title TEXT, archived_at TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('alpha', 'today')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO order_nulls_last (title, archived_at) VALUES ('beta', NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM order_nulls_last ORDER BY archived_at NULLS LAST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_outer (title TEXT)", vec![])

.unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE predicate_exists_inner (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_exists_inner (title) VALUES ('present')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_exists_outer WHERE EXISTS (SELECT title FROM predicate_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_empty_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_empty_exists");
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
                "CREATE TABLE predicate_empty_exists_outer (title TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_empty_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_empty_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_empty_exists_outer WHERE EXISTS (SELECT title FROM predicate_empty_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not");
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
                "CREATE TABLE predicate_not_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('keep')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_docs (title) VALUES ('skip')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_docs WHERE NOT title = 'skip'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("keep".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_rows_with_not_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("predicate_not_exists");
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
                "CREATE TABLE predicate_not_exists_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE predicate_not_exists_inner (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO predicate_not_exists_outer (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM predicate_not_exists_outer WHERE NOT EXISTS (SELECT title FROM predicate_not_exists_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_grouped_count_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_count");
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
                "CREATE TABLE aggregate_count_docs (category TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('b')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO aggregate_count_docs (category) VALUES ('a')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM aggregate_count_docs GROUP BY category ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("a".to_string()), Value::Int64(2)],
                vec![Value::String("b".to_string()), Value::Int64(1)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_numeric_aggregates_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_numeric");
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
                "CREATE TABLE aggregate_numeric_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (5)",
            "INSERT INTO aggregate_numeric_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_numeric_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(15),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_null_values_for_basic_aggregates_query() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_nulls");
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
                "CREATE TABLE aggregate_null_sales (amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO aggregate_null_sales (amount) VALUES (7)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (NULL)",
            "INSERT INTO aggregate_null_sales (amount) VALUES (3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(amount) AS present, SUM(amount) AS total, AVG(amount) AS average, MIN(amount) AS smallest, MAX(amount) AS largest FROM aggregate_null_sales",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::Int64(2),
                Value::Int64(10),
                Value::Float64(5.0),
                Value::Int64(3),
                Value::Int64(7)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_row_number_window_function_query() {
    // Arrange
    with_fallback();
    let path = data_dir("window_row_number");
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
                "CREATE TABLE window_scores (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'first', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('a', 'second', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_scores (category, title, score) VALUES ('b', 'third', 30)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, title, row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM window_scores ORDER BY category ASC, rank ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("second".to_string()),
                    Value::Int64(1)
                ],
                vec![
                    Value::String("a".to_string()),
                    Value::String("first".to_string()),
                    Value::Int64(2)
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("third".to_string()),
                    Value::Int64(1)
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_basic_value_window_functions_query() {
    // Arrange
    with_fallback();
    let path = data_dir("window_basic_values");
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
                "CREATE TABLE window_values (category TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'alpha', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO window_values (category, title, score) VALUES ('a', 'gamma', 20)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title, rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS rnk, dense_rank() OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS dense, lag(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS prev, lead(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS next, first_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS first, last_value(title) OVER (PARTITION BY category ORDER BY score DESC, title ASC) AS last FROM window_values ORDER BY rnk ASC, title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("alpha".to_string()),
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::Null,
                    Value::String("beta".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
                vec![
                    Value::String("beta".to_string()),
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string()),
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
                vec![
                    Value::String("gamma".to_string()),
                    Value::Int64(3),
                    Value::Int64(3),
                    Value::String("beta".to_string()),
                    Value::Null,
                    Value::String("alpha".to_string()),
                    Value::String("gamma".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_grouped_rows_with_having() {
    // Arrange
    with_fallback();
    let path = data_dir("aggregate_having");
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
                "CREATE TABLE aggregate_having_sales (category TEXT, amount INT)",
                vec![],
            )

.unwrap();
        for sql in [
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 7)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('a', 5)",
            "INSERT INTO aggregate_having_sales (category, amount) VALUES ('b', 3)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT category, SUM(amount) AS total FROM aggregate_having_sales GROUP BY category HAVING SUM(amount) > 10",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("a".to_string()), Value::Int64(12)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
