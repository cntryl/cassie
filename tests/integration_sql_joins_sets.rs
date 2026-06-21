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
fn should_apply_limit_offset_after_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("limit_offset_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_limit_offset_order";
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

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "pear", "body": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "apple", "body": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "banana", "body": "b"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_limit_offset_order ORDER BY title ASC LIMIT 2 OFFSET 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.columns[1].name, "title");
        assert_eq!(result.rows.len(), 2);

        let rows = result.rows;
        let ids = rows
            .iter()
            .map(|row| match &row[0] {
                cassie::types::Value::String(id) => id.clone(),
                _ => panic!("expected string id"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d3".to_string(), "d1".to_string()]);

        let titles = rows
            .iter()
            .map(|row| match &row[1] {
                cassie::types::Value::String(title) => title.clone(),
                _ => panic!("expected string title"),
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["banana".to_string(), "pear".to_string()]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_projected_scan_range_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_range");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_range";
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
                serde_json::json!({"title": "low", "score": 1}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "mid", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "high", "score": 20}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_projected_scan_range WHERE score >= 10 LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::String("mid".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[1][1], Value::String("high".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_filter_projected_scan_simple_equality_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_equality");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_equality";
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

        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "body": "first"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "second"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, title FROM sql_projected_scan_equality WHERE title = 'beta'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));
        assert_eq!(result.rows[0][1], Value::String("beta".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_vector_distance_offset_after_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_distance_offset_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_vector_distance_offset_order";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "embedding".to_string(),
                data_type: DataType::Vector(3),
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
                serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"embedding": [3.0, 0.0, 0.0]}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"embedding": [2.0, 0.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM sql_vector_distance_offset_order ORDER BY distance ASC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));
        assert_eq!(result.rows[0][1], Value::Float64(1.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_fulltext_offset_after_score_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_top_k_offset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_fulltext_top_k_offset";
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
                serde_json::json!({"body": "alpha alpha alpha"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "alpha alpha"}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM sql_fulltext_top_k_offset WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_hybrid_offset_after_score_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("hybrid_top_k_offset");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_hybrid_top_k_offset";
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
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"body": "red red", "embedding": [2.0, 0.0]}),
            )

            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_hybrid_top_k_offset ORDER BY score DESC LIMIT 1 OFFSET 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d3".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_hash_join_strategy_for_inner_equi_join() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_hash_join");
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
                "CREATE TABLE sql_hash_join_users (user_key TEXT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hash_join_orders (order_user_key TEXT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_hash_join_users.name, sql_hash_join_orders.total FROM sql_hash_join_users JOIN sql_hash_join_orders ON sql_hash_join_users.user_key = sql_hash_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=hash"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_semi_join_strategy_for_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_semi_join");
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
                "CREATE TABLE sql_semi_join_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_semi_join_inner (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_semi_join_outer WHERE EXISTS (SELECT title FROM sql_semi_join_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=semi"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_anti_join_strategy_for_not_exists_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_anti_join");
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
                "CREATE TABLE sql_anti_join_outer (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_anti_join_inner (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_anti_join_outer WHERE NOT EXISTS (SELECT title FROM sql_anti_join_inner)",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=anti"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_sql_with_non_recursive_cte() {
    // Arrange
    with_fallback();
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
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], cassie::types::Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_sql_with_recursive_cte() {
    // Arrange
    with_fallback();
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
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT n FROM counter WHERE n = 1) SELECT n FROM counter ORDER BY n",
            vec![],
            )

.unwrap();

        let rows = result
            .rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                _ => panic!("expected integer value"),
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(rows, vec![1]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_inner_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_inner");
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
                "CREATE TABLE join_users (user_key INT, name TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO join_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT join_users.name, join_orders.total FROM join_users JOIN join_orders ON join_users.user_key = join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Int64(42)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_left_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_left");
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
                "CREATE TABLE left_users (user_key INT, name TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE left_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO left_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT left_users.name, left_orders.total FROM left_users LEFT JOIN left_orders ON left_users.user_key = left_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Null]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_right_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_right");
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
                "CREATE TABLE right_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE right_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO right_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT right_users.name, right_orders.total FROM right_users RIGHT JOIN right_orders ON right_users.user_key = right_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::Null, Value::Int64(42)]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_full_outer_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_full_outer");
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
                "CREATE TABLE full_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE full_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO full_orders (order_user_key, total) VALUES (2, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT full_users.name, full_orders.total FROM full_users FULL OUTER JOIN full_orders ON full_users.user_key = full_orders.order_user_key ORDER BY full_users.name NULLS LAST",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Null],
                vec![Value::Null, Value::Int64(42)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_cross_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_cross");
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
                "CREATE TABLE cross_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cross_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cross_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT cross_users.name, cross_orders.total FROM cross_users CROSS JOIN cross_orders ORDER BY cross_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(42)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_lateral_join_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_lateral");
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
                "CREATE TABLE lateral_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE lateral_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_users (user_key, name) VALUES (2, 'grace')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (1, 99)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO lateral_orders (order_user_key, total) VALUES (2, 7)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT lateral_users.name, recent.total FROM lateral_users JOIN LATERAL (SELECT total FROM lateral_orders WHERE order_user_key = lateral_users.user_key ORDER BY total DESC LIMIT 1) AS recent ON true ORDER BY lateral_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("grace".to_string()), Value::Int64(7)]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_cross_apply_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_cross_apply");
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
                "CREATE TABLE apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO apply_orders (order_key, total) VALUES (10, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT apply_users.name, recent.total FROM apply_users CROSS APPLY (SELECT total FROM apply_orders) AS recent ORDER BY apply_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("ada".to_string()),
                Value::Int64(42)
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_outer_apply_query() {
    // Arrange
    with_fallback();
    let path = data_dir("join_outer_apply");
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
                "CREATE TABLE outer_apply_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE apply_missing_orders (order_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO outer_apply_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT outer_apply_users.name, recent.total FROM outer_apply_users OUTER APPLY (SELECT total FROM apply_missing_orders) AS recent ORDER BY outer_apply_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("ada".to_string()), Value::Null]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_from_subquery_query() {
    // Arrange
    with_fallback();
    let path = data_dir("from_subquery");
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
                "CREATE TABLE from_subquery_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO from_subquery_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT recent.title FROM (SELECT title FROM from_subquery_docs) AS recent",
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
fn should_execute_distinct_query() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_query");
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
                "CREATE TABLE distinct_docs (category TEXT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO distinct_docs (category) VALUES ('b')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
            "INSERT INTO distinct_docs (category) VALUES ('a')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category FROM distinct_docs ORDER BY category",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("a".to_string())],
                vec![Value::String("b".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_union_all_query() {
    // Arrange
    with_fallback();
    let path = data_dir("union_all_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_all_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE union_all_right (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_left (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_all_right (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_all_left UNION ALL SELECT title FROM union_all_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
                vec![Value::String("beta".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_union_query_with_deduplication() {
    // Arrange
    with_fallback();
    let path = data_dir("union_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_right (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_left (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO union_right (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_left UNION SELECT title FROM union_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_intersect_query() {
    // Arrange
    with_fallback();
    let path = data_dir("intersect_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE intersect_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE intersect_right (title TEXT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO intersect_left (title) VALUES ('alpha')",
            "INSERT INTO intersect_left (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('beta')",
            "INSERT INTO intersect_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM intersect_left INTERSECT SELECT title FROM intersect_right",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_except_query() {
    // Arrange
    with_fallback();
    let path = data_dir("except_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE except_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE except_right (title TEXT)", vec![])
            .unwrap();
        for sql in [
            "INSERT INTO except_left (title) VALUES ('alpha')",
            "INSERT INTO except_left (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('beta')",
            "INSERT INTO except_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM except_left EXCEPT SELECT title FROM except_right",
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
fn should_execute_distinct_on_query_with_ordering() {
    // Arrange
    with_fallback();
    let path = data_dir("distinct_on_query");
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
                "CREATE TABLE distinct_on_docs (tenant_id TEXT, title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'low', 1)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('a', 'high', 9)",
            "INSERT INTO distinct_on_docs (tenant_id, title, score) VALUES ('b', 'only', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM distinct_on_docs ORDER BY tenant_id ASC, score DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("a".to_string()),
                    Value::String("high".to_string())
                ],
                vec![
                    Value::String("b".to_string()),
                    Value::String("only".to_string())
                ]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_order_limit_offset_after_union_all() {
    // Arrange
    with_fallback();
    let path = data_dir("union_global_order");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE union_order_right (title TEXT)", vec![])
            .unwrap();
        for sql in [
            "INSERT INTO union_order_left (title) VALUES ('beta')",
            "INSERT INTO union_order_right (title) VALUES ('alpha')",
            "INSERT INTO union_order_right (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_order_left UNION ALL SELECT title FROM union_order_right ORDER BY title LIMIT 1 OFFSET 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_chained_union_all_query() {
    // Arrange
    with_fallback();
    let path = data_dir("union_all_chained");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for table in ["union_chain_a", "union_chain_b", "union_chain_c"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {table} (title TEXT)"), vec![])
                .unwrap();
        }
        for sql in [
            "INSERT INTO union_chain_a (title) VALUES ('alpha')",
            "INSERT INTO union_chain_b (title) VALUES ('beta')",
            "INSERT INTO union_chain_c (title) VALUES ('gamma')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM union_chain_a UNION ALL SELECT title FROM union_chain_b UNION ALL SELECT title FROM union_chain_c ORDER BY title DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("gamma".to_string())],
                vec![Value::String("beta".to_string())],
                vec![Value::String("alpha".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
