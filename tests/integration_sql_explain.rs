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
fn should_report_read_path_metadata_in_explain() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_read_path_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_explain_read_path_metadata";
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
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, title FROM sql_explain_read_path_metadata WHERE id = 'doc-1'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=point_lookup"));
        assert!(plan.contains("access_path_reason=point-lookup-id"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(plan.contains("pagination_strategy=none"));
        assert!(plan.contains("top_k_mode=none"));
        assert!(plan.contains("early_stop=point_lookup"));
        assert!(plan.contains("projection_shape=materialized_projection"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_materialized_projection_freshness() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_materialized_projection");
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
                "CREATE TABLE sql_explain_projection_source (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_explain_projection_source (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION sql_explain_projection_ready AS SELECT title, score FROM sql_explain_projection_source",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = explain_plan_text(&result);
        assert_explain_contains(plan, "projection_freshness", "fresh");

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
        assert!(plan.contains("early_stop=scan_limit"));

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
        assert!(plan.contains("top_k_mode=heap"));
        assert!(plan.contains("early_stop=none"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_storage_top_k_read_path_for_row_id_order() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_storage_top_k");
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
                "CREATE TABLE sql_explain_storage_top_k (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, title FROM sql_explain_storage_top_k ORDER BY id ASC LIMIT 5",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = explain_plan_text(&result);
        assert_explain_contains(plan, "access_path_reason", "row-key-top-k");
        assert_explain_contains(plan, "pagination_strategy", "limit");
        assert_explain_contains(plan, "top_k_mode", "storage");
        assert_explain_contains(plan, "early_stop", "storage_top_k");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_keyset_read_path_for_row_id_cursor() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_keyset");
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
                "CREATE TABLE sql_explain_keyset (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, title FROM sql_explain_keyset WHERE id > 'doc-1' ORDER BY id ASC LIMIT 5",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = explain_plan_text(&result);
        assert_explain_contains(plan, "access_path_reason", "row-key-keyset");
        assert_explain_contains(plan, "pagination_strategy", "keyset");
        assert_explain_contains(plan, "top_k_mode", "none");
        assert_explain_contains(plan, "early_stop", "keyset");

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
