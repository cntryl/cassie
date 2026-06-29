#![allow(unused_imports, dead_code)]

use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use tokio_postgres::NoTls;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn vectorized_join_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config
}

#[test]
fn should_lock_scalar_read_model_hot_path_access_paths() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_hot_paths");
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
                "CREATE TABLE read_model_hot_paths \
                 (tenant TEXT, status TEXT, created_at INT, title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (id, tenant, status, created_at, title, body) in [
            ("row-1", "tenant-a", "open", 30, "Gamma", "third"),
            ("row-2", "tenant-a", "open", 10, "Alpha", "first"),
            ("row-3", "tenant-a", "open", 20, "Beta", "second"),
            ("row-4", "tenant-a", "closed", 5, "Closed", "closed"),
            ("row-5", "tenant-b", "open", 1, "Other", "other"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_model_hot_paths",
                    Some(id.to_string()),
                    serde_json::json!({
                        "tenant": tenant,
                        "status": status,
                        "created_at": created_at,
                        "title": title,
                        "body": body
                    }),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_hot_paths_tenant_status_time_idx \
                 ON read_model_hot_paths USING btree (tenant, status, created_at)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_hot_paths_lower_title_idx \
                 ON read_model_hot_paths USING btree (lower(title))",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let page = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let page_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let expression = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let expression_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            page.rows,
            vec![
                vec![Value::String("first".to_string())],
                vec![Value::String("second".to_string())],
            ]
        );
        assert_eq!(
            expression.rows,
            vec![vec![Value::String("first".to_string())]]
        );

        let page_plan = explain_plan_text(&page_explain);
        assert_explain_contains(
            page_plan,
            "index",
            "read_model_hot_paths_tenant_status_time_idx",
        );
        assert_explain_contains(page_plan, "access_path", "range_scan");
        assert_explain_contains(page_plan, "fallback_reason", "none");

        let expression_plan = explain_plan_text(&expression_explain);
        assert_explain_contains(
            expression_plan,
            "index",
            "read_model_hot_paths_lower_title_idx",
        );
        assert_explain_contains(expression_plan, "access_path", "index_seek");
        assert_explain_contains(expression_plan, "fallback_reason", "none");

        assert!(
            after["read_paths"]["range_scans"].as_u64().unwrap()
                > before["read_paths"]["range_scans"].as_u64().unwrap()
        );
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_column_batch_covered_read_hot_path() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_column_batch");
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
                "CREATE TABLE read_model_column_batch \
                 (title TEXT, body TEXT, status TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for (title, body, status, score) in [
            ("alpha", "one", "approved", 10),
            ("beta", "two", "pending", 20),
            ("gamma", "three", "approved", 30),
        ] {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO read_model_column_batch \
                         (title, body, status, score) VALUES \
                         ('{title}', '{body}', '{status}', {score})"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_column_batch_idx \
                 ON read_model_column_batch USING column (title, body, status, score) \
                 WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("alpha".to_string()),
                    Value::String("one".to_string())
                ],
                vec![
                    Value::String("gamma".to_string()),
                    Value::String("three".to_string())
                ],
            ]
        );
        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "column_batch_index", "read_model_column_batch_idx");
        assert!(
            after["column_batches"]["scans"].as_u64().unwrap()
                > before["column_batches"]["scans"].as_u64().unwrap()
        );
        assert!(
            after["column_batches"]["row_fetches_avoided"]
                .as_u64()
                .unwrap()
                >= before["column_batches"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap()
                    + 2
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_vectorized_join_hot_path_when_enabled() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_vectorized_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO read_model_join_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO read_model_join_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (2, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key \
                 ORDER BY read_model_join_users.name",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(7)],
            ]
        );
        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "vectorized_join_candidate", "true");
        assert_explain_contains(plan, "vectorized_join_enabled", "true");
        assert_explain_contains(plan, "vectorized_join_fallback_reason", "none");
        assert!(
            after["joins"]["vectorized_joins"].as_u64().unwrap()
                > before["joins"]["vectorized_joins"].as_u64().unwrap()
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stop_vectorized_join_after_unordered_limit_budget() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_vectorized_join_limit_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_limit_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_limit_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        let mut orders = Vec::new();
        for index in 0..64 {
            users.push((
                Some(format!("user-{index:02}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:02}"),
                }),
            ));
            orders.push((
                Some(format!("order-{index:02}")),
                serde_json::json!({
                    "order_user_key": i64::from(index),
                    "total": i64::from(index),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents("read_model_limit_users", users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents("read_model_limit_orders", orders)
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_limit_users.name, read_model_limit_orders.total \
                 FROM read_model_limit_users JOIN read_model_limit_orders \
                 ON read_model_limit_users.user_key = read_model_limit_orders.order_user_key \
                 LIMIT 5",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 5);
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let output_delta = after["joins"]["output_rows_total"].as_u64().unwrap()
            - before["joins"]["output_rows_total"].as_u64().unwrap();
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        assert_eq!(output_delta, 5);
        assert!(
            probe_delta <= 8,
            "expected limited join to probe at most one vectorized batch, got {probe_delta}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_push_unordered_left_join_limit_into_left_source_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_left_join_source_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.temp_spill_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_budget_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_budget_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents("read_model_budget_users", users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                "read_model_budget_orders",
                vec![(
                    Some("order-000".to_string()),
                    serde_json::json!({
                        "order_user_key": 0_i64,
                        "total": 42_i64,
                    }),
                )],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_budget_users.name, read_model_budget_orders.total \
                 FROM read_model_budget_users LEFT JOIN read_model_budget_orders \
                 ON read_model_budget_users.user_key = read_model_budget_orders.order_user_key \
                 LIMIT 5",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 5);
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        assert!(
            probe_delta <= 8,
            "expected limited left join to scan at most one left-source batch, got {probe_delta}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_probe_indexed_left_source_for_bounded_inner_join() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_indexed_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.temp_spill_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_indexed_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_indexed_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents("read_model_indexed_users", users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                "read_model_indexed_orders",
                vec![
                    (
                        Some("order-150".to_string()),
                        serde_json::json!({
                            "order_user_key": 150_i64,
                            "total": 150_i64,
                        }),
                    ),
                    (
                        Some("order-151".to_string()),
                        serde_json::json!({
                            "order_user_key": 151_i64,
                            "total": 151_i64,
                        }),
                    ),
                ],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_indexed_users_key_idx \
                 ON read_model_indexed_users USING btree (user_key)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_indexed_users.name, read_model_indexed_orders.total \
                 FROM read_model_indexed_users JOIN read_model_indexed_orders \
                 ON read_model_indexed_users.user_key = read_model_indexed_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-150".to_string()), Value::Int64(150)],
                vec![Value::String("user-151".to_string()), Value::Int64(151)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        let index_seek_delta = after["read_paths"]["index_seek_scans"].as_u64().unwrap()
            - before["read_paths"]["index_seek_scans"].as_u64().unwrap();
        assert!(
            probe_delta <= 2,
            "expected indexed bounded join to probe only matching left rows, got {probe_delta}"
        );
        assert!(
            index_seek_delta > 0,
            "expected bounded inner join to use the indexed left source"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stream_unindexed_bounded_inner_join_until_output_budget() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_streaming_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.temp_spill_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents("read_model_stream_users", users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                "read_model_stream_orders",
                vec![
                    (
                        Some("order-000".to_string()),
                        serde_json::json!({
                            "order_user_key": 0_i64,
                            "total": 10_i64,
                        }),
                    ),
                    (
                        Some("order-001".to_string()),
                        serde_json::json!({
                            "order_user_key": 1_i64,
                            "total": 11_i64,
                        }),
                    ),
                ],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_stream_users.name, read_model_stream_orders.total \
                 FROM read_model_stream_users JOIN read_model_stream_orders \
                 ON read_model_stream_users.user_key = read_model_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(10)],
                vec![Value::String("user-001".to_string()), Value::Int64(11)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 10,
            "expected streaming bounded join to avoid full left materialization, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stream_dense_bounded_inner_join_without_materializing_right_source() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_dense_streaming_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        config.limits.temp_spill_budget_bytes = 4 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        let mut orders = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
            orders.push((
                Some(format!("order-{index:03}")),
                serde_json::json!({
                    "order_user_key": 0_i64,
                    "total": i64::from(index),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents("read_model_dense_stream_users", users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents("read_model_dense_stream_orders", orders)
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_dense_stream_users.name, read_model_dense_stream_orders.total \
                 FROM read_model_dense_stream_users JOIN read_model_dense_stream_orders \
                 ON read_model_dense_stream_users.user_key = read_model_dense_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(0)],
                vec![Value::String("user-000".to_string()), Value::Int64(1)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 6,
            "expected dense bounded join to avoid materializing either source, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_pgwire_prepared_read_hot_path_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("read_model_pgwire_prepared");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_pgwire_prepared (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO read_model_pgwire_prepared (title, score) VALUES ('alpha', 7)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_pgwire_prepared_score_idx \
                 ON read_model_pgwire_prepared USING btree (score)",
                vec![],
            )
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let before = cassie.metrics();
        let mut client_config = tokio_postgres::Config::new();
        client_config.host("127.0.0.1");
        client_config.port(addr.port());
        client_config.user("postgres");
        client_config.dbname("postgres");
        let (client, connection) = client_config
            .connect(NoTls)
            .await
            .expect("connect tokio-postgres");
        let connection = tokio::spawn(async move {
            connection
                .await
                .expect("tokio-postgres connection should stay healthy");
        });

        // Act
        let statement = client
            .prepare("SELECT title FROM read_model_pgwire_prepared WHERE score = $1")
            .await
            .expect("prepare statement");
        let row = client
            .query_one(&statement, &[&7_i32])
            .await
            .expect("execute prepared statement");
        let title: String = row.try_get(0).expect("title value");
        drop(client);
        let _ = connection.await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let after = cassie.metrics();

        // Assert
        assert_eq!(title, "alpha");
        assert!(
            after["pgwire"]["extended_queries_total"].as_u64().unwrap()
                > before["pgwire"]["extended_queries_total"].as_u64().unwrap()
        );
        assert_eq!(
            after["pgwire"]["protocol_errors_total"].as_u64(),
            before["pgwire"]["protocol_errors_total"].as_u64()
        );
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
