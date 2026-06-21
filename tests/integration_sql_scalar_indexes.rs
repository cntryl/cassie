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
