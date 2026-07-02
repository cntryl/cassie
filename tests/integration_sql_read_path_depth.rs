#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_scan_mixed_order_suffix_when_prefix_order_field_is_equality_bound() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_mixed_order_prefix");
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
                "CREATE TABLE read_path_mixed_order_prefix \
                 (tenant TEXT, status TEXT, created_at INT, title TEXT)",
                vec![],
            )
            .unwrap();
        for (id, tenant, status, created_at, title) in [
            ("row-1", "tenant-a", "open", 30, "third"),
            ("row-2", "tenant-a", "open", 10, "first"),
            ("row-3", "tenant-a", "open", 20, "second"),
            ("row-4", "tenant-a", "closed", 5, "closed"),
            ("row-5", "tenant-b", "open", 1, "other"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_path_mixed_order_prefix",
                    Some(id.to_string()),
                    serde_json::json!({
                        "tenant": tenant,
                        "status": status,
                        "created_at": created_at,
                        "title": title
                    }),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_path_mixed_order_prefix_idx \
                 ON read_path_mixed_order_prefix USING btree (tenant, status, created_at)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM read_path_mixed_order_prefix \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM read_path_mixed_order_prefix \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("first".to_string())],
                vec![Value::String("second".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=read_path_mixed_order_prefix_idx"));
        assert!(plan.contains("access_path=range_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(
            after["read_paths"]["range_scans"].as_u64().unwrap()
                > before["read_paths"]["range_scans"].as_u64().unwrap()
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("read_path_mixed_order_prefix_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_index_prefix_then_sort_mixed_direction_suffix() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_mixed_order_suffix");
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
                "CREATE TABLE read_path_mixed_order_suffix \
                 (tenant TEXT, created_at INT, priority INT, title TEXT)",
                vec![],
            )
            .unwrap();
        for (id, tenant, created_at, priority, title) in [
            ("row-1", "tenant-a", 30, 5, "late-high"),
            ("row-2", "tenant-a", 30, 1, "late-low"),
            ("row-3", "tenant-a", 20, 1, "mid-low"),
            ("row-4", "tenant-a", 20, 9, "mid-high"),
            ("row-5", "tenant-b", 99, 1, "other"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_path_mixed_order_suffix",
                    Some(id.to_string()),
                    serde_json::json!({
                        "tenant": tenant,
                        "created_at": created_at,
                        "priority": priority,
                        "title": title
                    }),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_path_mixed_order_suffix_idx \
                 ON read_path_mixed_order_suffix USING btree (tenant, created_at, priority)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM read_path_mixed_order_suffix \
                 WHERE tenant = 'tenant-a' \
                 ORDER BY created_at DESC, priority ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM read_path_mixed_order_suffix \
                 WHERE tenant = 'tenant-a' \
                 ORDER BY created_at DESC, priority ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("late-low".to_string())],
                vec![Value::String("late-high".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=read_path_mixed_order_suffix_idx"));
        assert!(plan.contains("access_path=prefix_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-prefix"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(plan.contains("top_k_mode=heap"));
        assert!(plan.contains("early_stop=none"));
        assert!(
            after["read_paths"]["prefix_scans"].as_u64().unwrap()
                > before["read_paths"]["prefix_scans"].as_u64().unwrap()
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("read_path_mixed_order_suffix_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_expression_index_after_restart_with_row_blob_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_expression_index_restart");
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
                    "CREATE TABLE read_path_expression_index_restart (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "read_path_expression_index_restart",
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "Alpha", "body": "kept in row blob"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "read_path_expression_index_restart",
                    Some("row-2".to_string()),
                    serde_json::json!({"title": "Beta", "body": "filtered"}),
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX read_path_expression_index_restart_idx \
                     ON read_path_expression_index_restart USING btree (lower(title))",
                    vec![],
                )
                .unwrap();
        }

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let before = restarted.metrics();

        // Act
        let result = restarted
            .execute_sql(
                &session,
                "SELECT body FROM read_path_expression_index_restart WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_path_expression_index_restart WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let after = restarted.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("kept in row blob".to_string())]]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=read_path_expression_index_restart_idx"));
        assert!(plan.contains("access_path=index_seek"));
        assert!(plan.contains("access_path_reason=scalar-index-seek"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("read_path_expression_index_restart_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_expression_index_range_with_row_blob_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_expression_index_range");
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
                "CREATE TABLE read_path_expression_index_range (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (id, title, body) in [
            ("row-1", "Alpha", "below"),
            ("row-2", "Omega", "inside"),
            ("row-3", "Zulu", "above"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_path_expression_index_range",
                    Some(id.to_string()),
                    serde_json::json!({"title": title, "body": body}),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_path_expression_index_range_idx \
                 ON read_path_expression_index_range USING btree (lower(title))",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_path_expression_index_range \
                 WHERE lower(title) >= 'm' AND lower(title) < 'z'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_path_expression_index_range \
                 WHERE lower(title) >= 'm' AND lower(title) < 'z'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=read_path_expression_index_range_idx"));
        assert!(plan.contains("access_path=range_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(
            after["read_paths"]["range_scans"].as_u64().unwrap()
                > before["read_paths"]["range_scans"].as_u64().unwrap()
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("read_path_expression_index_range_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_expression_index_order_limit_with_row_blob_projection() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_expression_index_order");
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
                "CREATE TABLE read_path_expression_index_order (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (id, title, body) in [
            ("row-1", "Beta", "second"),
            ("row-2", "alpha", "third"),
            ("row-3", "Gamma", "first"),
        ] {
            cassie
                .midge
                .put_document(
                    "read_path_expression_index_order",
                    Some(id.to_string()),
                    serde_json::json!({"title": title, "body": body}),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_path_expression_index_order_idx \
                 ON read_path_expression_index_order USING btree (lower(title))",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_path_expression_index_order \
                 ORDER BY lower(title) DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_path_expression_index_order \
                 ORDER BY lower(title) DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("first".to_string())],
                vec![Value::String("second".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=read_path_expression_index_order_idx"));
        assert!(plan.contains("access_path=ordered_bounded_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-ordered-bounded"));
        assert!(plan.contains("fallback_reason=none"));
        assert!(plan.contains("top_k_mode=storage"));
        assert!(
            after["read_paths"]["ordered_bounded_scans"]
                .as_u64()
                .unwrap()
                > before["read_paths"]["ordered_bounded_scans"]
                    .as_u64()
                    .unwrap()
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("read_path_expression_index_order_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
