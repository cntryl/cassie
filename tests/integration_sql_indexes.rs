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
fn should_explain_index_aware_plan_for_scalar_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_index_aware");
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
                "CREATE TABLE sql_explain_index_aware (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_index_aware_email_idx ON sql_explain_index_aware USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_index_aware WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index_aware=true"));
        assert!(plan.contains("index=sql_explain_index_aware_email_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_covering_scalar_index_for_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("covering_scalar_projection");
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
                "CREATE TABLE sql_covering_scalar_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_projection (email, title) VALUES ('a@example.com', 'alpha'), ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_projection_email_idx ON sql_covering_scalar_projection USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("a@example.com".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_projection_email_idx"));
        assert!(plan.contains("covered_index=true"));
        assert_eq!(
            after["covering_indexes"]["scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["covering_indexes"]["row_fetches_avoided"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fallback_for_noncovered_scalar_index_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("covering_scalar_fallback");
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
                "CREATE TABLE sql_covering_scalar_fallback (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_fallback (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_fallback_email_idx ON sql_covering_scalar_fallback USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_fallback_email_idx"));
        assert!(plan.contains("covered_index=false"));
        assert_eq!(
            after["covering_indexes"]["fallback_scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["fallback_scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_covering_scalar_index_order_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("covering_scalar_restart_order");
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
                    "CREATE TABLE sql_covering_scalar_restart_order (email TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "sql_covering_scalar_restart_order",
                    Some("doc-2".to_string()),
                    serde_json::json!({"email": "a@example.com", "title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "sql_covering_scalar_restart_order",
                    Some("doc-1".to_string()),
                    serde_json::json!({"email": "a@example.com"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "sql_covering_scalar_restart_order",
                    Some("doc-3".to_string()),
                    serde_json::json!({"email": "b@example.com", "title": "beta"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_covering_scalar_restart_order_email_idx ON sql_covering_scalar_restart_order USING btree (email)",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, email FROM sql_covering_scalar_restart_order WHERE email = 'a@example.com' ORDER BY id DESC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("doc-2".to_string()),
                    Value::String("a@example.com".to_string())
                ],
                vec![
                    Value::String("doc-1".to_string()),
                    Value::String("a@example.com".to_string())
                ],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_include_index_metadata_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("include_metadata_restart");
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
                    "CREATE TABLE sql_include_metadata_restart (email TEXT, title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_include_metadata_restart_email_idx ON sql_include_metadata_restart USING btree (email) INCLUDE (title, body)",
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
                "sql_include_metadata_restart",
                "sql_include_metadata_restart_email_idx",
            )
            .expect("index should hydrate");
        let session = restarted.create_session("tester", None);
        let introspection = restarted
            .execute_sql(
                &session,
                "SELECT indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'sql_include_metadata_restart'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            index.include_fields,
            vec!["title".to_string(), "body".to_string()]
        );
        let Value::String(indexdef) = &introspection.rows[0][0] else {
            panic!("expected textual index definition");
        };
        assert!(indexdef.contains("INCLUDE (title, body)"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_include_columns_for_covered_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("include_covered_projection");
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
                "CREATE TABLE sql_include_covered_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_include_covered_projection (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_include_covered_projection_email_idx ON sql_include_covered_projection USING btree (email) INCLUDE (title)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_partial_index_metadata_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("partial_index_restart");
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
                    "CREATE TABLE sql_partial_index_restart (title TEXT, status TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX sql_partial_index_restart_title_idx ON sql_partial_index_restart USING btree (title) WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let index = restarted
            .catalog
            .get_index(
                "sql_partial_index_restart",
                "sql_partial_index_restart_title_idx",
            )
            .expect("partial index should hydrate");
        let selected = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let fallback = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_partial_index_restart WHERE title = 'beta'",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(index.predicate.is_some());
        let Value::String(selected_plan) = &selected.rows[0][0] else {
            panic!("expected selected plan text");
        };
        assert!(selected_plan.contains("index=sql_partial_index_restart_title_idx"));
        let Value::String(fallback_plan) = &fallback.rows[0][0] else {
            panic!("expected fallback plan text");
        };
        assert!(fallback_plan.contains("index=none"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_vector_prefilter_for_indexed_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_vector_prefilter_indexed");
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
                "CREATE TABLE sql_explain_vector_prefilter_indexed (status TEXT, embedding VECTOR(2), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_vector_prefilter_status_idx ON sql_explain_vector_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_vector_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0], "title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_explain_vector_prefilter_indexed WHERE status = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_vector_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_vector_metadata_prefilter_for_supported_predicates() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_supported_predicates");
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
                "CREATE TABLE sql_vector_prefilter_supported_predicates (status TEXT, rating INT, category TEXT, archived_at TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d2".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d3".to_string()),
                serde_json::json!({"status": "approved", "rating": 3, "category": "alpha", "archived_at": null, "embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d4".to_string()),
                serde_json::json!({"status": "pending", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_supported_predicates WHERE (status = 'approved') AND (rating BETWEEN 4 AND 6) AND (category IN ('alpha', 'beta')) AND archived_at IS NULL ORDER BY distance ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value == 0.0));
        assert!(matches!(result.rows[1][1], Value::Float64(value) if value == 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unsupported_vector_metadata_predicate_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_unsupported_predicate");
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
                "CREATE TABLE sql_vector_prefilter_unsupported_predicate (status TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d2".to_string()),
                serde_json::json!({"status": "pending", "embedding": [0.0, 1.0]}),
            )
            .unwrap();

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=fallback=unsupported metadata predicate"));
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_hybrid_prefilter_for_indexed_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_hybrid_prefilter_indexed");
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
                "CREATE TABLE sql_explain_hybrid_prefilter_indexed (status TEXT, body TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_hybrid_prefilter_status_idx ON sql_explain_hybrid_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_hybrid_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "body": "red", "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_explain_hybrid_prefilter_indexed WHERE status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_hybrid_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_query_rows_after_creating_secondary_index() {
    // Arrange
    with_fallback();
    let path = data_dir("secondary_index_query");
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
                "CREATE TABLE secondary_index_query (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX secondary_email_idx ON secondary_index_query USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM secondary_index_query WHERE email = 'b@example.com'",
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
fn should_query_rows_after_creating_composite_index() {
    // Arrange
    with_fallback();
    let path = data_dir("composite_index_query");
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
                "CREATE TABLE composite_index_query (tenant_id TEXT, status TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX composite_tenant_status_idx ON composite_index_query USING btree (tenant_id, status)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'open', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'closed', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-b', 'closed', 'gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
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
fn should_reject_vector_index_when_embedding_dimensions_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_index_embedding_dimension_mismatch");
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
                "CREATE TABLE vector_index_embedding_dimension_mismatch (content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        // Act
        let created = cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_index_embedding_dimension_mismatch_idx ON vector_index_embedding_dimension_mismatch USING vector (embedding) WITH (source_field = content)",
                vec![],
            );

        // Assert
        assert!(created.is_err());
        assert!(created
            .unwrap_err()
            .to_string()
            .contains("embedding dimension mismatch"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_hnsw_vector_index_options_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_vector_index_options");
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hnsw_vector_index_options (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_hnsw_vector_index_options_idx ON sql_hnsw_vector_index_options USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 12, ef_construction = 96, ef_search = 48)",
                vec![],
            )
            .unwrap();
    }

    // Act
    let restarted =
        Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    restarted.startup().unwrap();
    let index = restarted
        .catalog
        .get_vector_index("sql_hnsw_vector_index_options", "embedding")
        .expect("hnsw vector index should hydrate");

    // Assert
    assert_eq!(index.metadata.index_type, VectorIndexType::Hnsw);
    let hnsw = index.metadata.hnsw.expect("hnsw options");
    assert_eq!(hnsw.m, 12);
    assert_eq!(hnsw.ef_construction, 96);
    assert_eq!(hnsw.ef_search, 48);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_normalized_vector_sidecars_after_sql_writes() {
    // Arrange
    with_fallback();
    let path = data_dir("normalized_sidecar_sql_rebuild");
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
                "CREATE TABLE normalized_sidecar_sql_rebuild (title TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        let row_id = match &cassie
            .execute_sql(
                &session,
                "INSERT INTO normalized_sidecar_sql_rebuild (title, embedding) VALUES ('alpha', $1) RETURNING _id",
                vec![Value::Vector(Vector::new(vec![3.0, 4.0, 0.0]))],
            )
            .unwrap()
            .rows[0][0]
        {
            Value::String(id) => id.clone(),
            other => panic!("expected string row id, got {other:?}"),
        };
        cassie
            .execute_sql(
                &session,
                "UPDATE normalized_sidecar_sql_rebuild SET embedding = $1 WHERE title = 'alpha'",
                vec![Value::Vector(Vector::new(vec![0.0, 0.0, 5.0]))],
            )
            .unwrap();

        let vector_index = VectorIndexRecord {
            collection: "normalized_sidecar_sql_rebuild".to_string(),
            field: "embedding".to_string(),
            source_field: "title".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
                index_type: VectorIndexType::BruteForce,
                hnsw: None,
            },
        };

        // Act
        cassie.midge.put_vector_index(vector_index.clone()).unwrap();
        let stored = cassie
            .midge
            .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
            .unwrap()
            .unwrap();

        clear_normalized_sidecars(&cassie, "normalized_sidecar_sql_rebuild", "embedding");
        assert!(
            cassie
                .midge
                .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
                .unwrap()
                .is_none()
        );

        cassie
            .midge
            .rebuild_normalized_vectors_for_index(&vector_index)
            .unwrap();
        let rebuilt = cassie
            .midge
            .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(stored.collection, "normalized_sidecar_sql_rebuild");
        assert_eq!(stored.field, "embedding");
        assert_eq!(stored.id, row_id);
        assert_eq!(stored.dimensions, 3);
        assert_eq!(stored.metric, DistanceMetric::Cosine);
        assert!(stored.payload_available);
        assert_eq!(stored.normalization_version, 1);
        assert_eq!(stored.values, vec![0.0, 0.0, 1.0]);
        assert_eq!(stored.magnitude, 5.0);
        assert_eq!(rebuilt.values, stored.values);
        assert_eq!(rebuilt.magnitude, stored.magnitude);

        let _ = std::fs::remove_dir_all(path);
    });
}
