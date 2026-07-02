#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::CollectionCardinalityStats;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn vectorized_join_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 8;
    config.limits.temp_spill_budget_bytes = 1024 * 1024;
    config
}

fn hydrate_cardinality(cassie: &Cassie, collection: &str) {
    let stats = cassie
        .midge
        .rebuild_cardinality_stats_for_collection(collection)
        .expect("cardinality stats");
    cassie.catalog.hydrate_cardinality_stats(collection, stats);
}

fn hydrate_row_count_only(cassie: &Cassie, collection: &str, row_count: u64) {
    cassie.catalog.hydrate_cardinality_stats(
        collection,
        CollectionCardinalityStats {
            row_count,
            ..CollectionCardinalityStats::default()
        },
    );
}

fn metric_delta(after: &serde_json::Value, before: &serde_json::Value, path: &[&str]) -> u64 {
    let after_value = path.iter().fold(after, |value, key| &value[*key]);
    let before_value = path.iter().fold(before, |value, key| &value[*key]);
    after_value.as_u64().unwrap() - before_value.as_u64().unwrap()
}

fn create_join_tables(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    users_table: &str,
    orders_table: &str,
) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {users_table} (user_key INT, name TEXT)"),
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {orders_table} (order_user_key INT, total INT)"),
            vec![],
        )
        .unwrap();
}

fn put_users(cassie: &Cassie, table: &str, count: usize) {
    let users = (0..count)
        .map(|index| {
            (
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::try_from(index).unwrap(),
                    "name": format!("user-{index:03}"),
                }),
            )
        })
        .collect();
    cassie.midge.put_fresh_documents(table, users).unwrap();
}

fn put_users_with_keys(cassie: &Cassie, table: &str, keys: &[i64]) {
    let users = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            (
                Some(format!("user-{index:04}")),
                serde_json::json!({
                    "user_key": *key,
                    "name": format!("user-{index:04}"),
                }),
            )
        })
        .collect();
    cassie.midge.put_fresh_documents(table, users).unwrap();
}

fn put_orders(cassie: &Cassie, table: &str, keys: &[i64]) {
    let orders = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            (
                Some(format!("order-{index:03}")),
                serde_json::json!({
                    "order_user_key": *key,
                    "total": *key,
                }),
            )
        })
        .collect();
    cassie.midge.put_fresh_documents(table, orders).unwrap();
}

fn select_inner_sql(users_table: &str, orders_table: &str, limit: usize) -> String {
    format!(
        "SELECT {users_table}.name, {orders_table}.total \
         FROM {users_table} JOIN {orders_table} \
         ON {users_table}.user_key = {orders_table}.order_user_key \
         LIMIT {limit}"
    )
}

#[test]
fn should_sample_row_count_stats_to_reduce_larger_bounded_join_without_field_stats() {
    // Arrange
    with_fallback();
    let path = data_dir("row_count_sample_bounded_inner_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(
            &cassie,
            &session,
            "row_count_sample_users",
            "row_count_sample_orders",
        );
        let user_keys = (0..1_000)
            .map(|index| i64::from(index % 10))
            .collect::<Vec<_>>();
        let order_keys = (0..3_000)
            .map(|index| i64::from(index % 10))
            .collect::<Vec<_>>();
        put_users_with_keys(&cassie, "row_count_sample_users", &user_keys);
        put_orders(&cassie, "row_count_sample_orders", &order_keys);
        hydrate_row_count_only(&cassie, "row_count_sample_users", 1_000);
        hydrate_row_count_only(&cassie, "row_count_sample_orders", 3_000);
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("row_count_sample_users", "row_count_sample_orders", 500),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 500);
        assert_eq!(
            metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
            1_000
        );
        assert!(metric_delta(&after, &before, &["joins", "vectorized_probe_rows_total"]) <= 5);
        assert_eq!(
            after["read_paths"]["last_collection_scan_collection"].as_str(),
            Some("row_count_sample_orders")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_fanout_stats_to_reduce_larger_bounded_join_build_side() {
    // Arrange
    with_fallback();
    let path = data_dir("fanout_stats_bounded_inner_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(
            &cassie,
            &session,
            "fanout_small_users",
            "fanout_large_orders",
        );
        let user_keys = (0..1_000)
            .map(|index| i64::from(index % 10))
            .collect::<Vec<_>>();
        let order_keys = (0..3_000)
            .map(|index| i64::from(index % 10))
            .collect::<Vec<_>>();
        put_users_with_keys(&cassie, "fanout_small_users", &user_keys);
        put_orders(&cassie, "fanout_large_orders", &order_keys);
        hydrate_cardinality(&cassie, "fanout_small_users");
        hydrate_cardinality(&cassie, "fanout_large_orders");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("fanout_small_users", "fanout_large_orders", 500),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 500);
        assert_eq!(
            metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
            1_000
        );
        assert!(metric_delta(&after, &before, &["joins", "vectorized_probe_rows_total"]) <= 5);
        assert_eq!(
            after["read_paths"]["last_collection_scan_collection"].as_str(),
            Some("fanout_large_orders")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_probe_indexed_right_source_for_bounded_inner_join() {
    // Arrange
    with_fallback();
    let path = data_dir("right_indexed_bounded_inner_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(
            &cassie,
            &session,
            "bounded_right_idx_users",
            "bounded_right_idx_orders",
        );
        put_users(&cassie, "bounded_right_idx_users", 4);
        put_orders(&cassie, "bounded_right_idx_orders", &[0, 1, 2, 3]);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX bounded_right_idx_orders_key_idx \
                 ON bounded_right_idx_orders USING btree (order_user_key)",
                vec![],
            )
            .unwrap();
        hydrate_cardinality(&cassie, "bounded_right_idx_users");
        hydrate_cardinality(&cassie, "bounded_right_idx_orders");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("bounded_right_idx_users", "bounded_right_idx_orders", 2),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(0)],
                vec![Value::String("user-001".to_string()), Value::Int64(1)],
            ]
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_collection"].as_str(),
            Some("bounded_right_idx_orders")
        );
        assert_eq!(
            after["read_paths"]["last_index_scan_index"].as_str(),
            Some("bounded_right_idx_orders_key_idx")
        );
        assert!(metric_delta(&after, &before, &["read_paths", "index_seek_scans"]) > 0);
        assert!(metric_delta(&after, &before, &["read_paths", "collection_scan_rows"]) <= 2);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_existing_left_index_probe_when_estimates_tie() {
    // Arrange
    with_fallback();
    let path = data_dir("indexed_bounded_tie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(&cassie, &session, "bounded_tie_users", "bounded_tie_orders");
        put_users(&cassie, "bounded_tie_users", 4);
        put_orders(&cassie, "bounded_tie_orders", &[0, 1, 2, 3]);
        for sql in [
            "CREATE INDEX bounded_tie_users_key_idx \
             ON bounded_tie_users USING btree (user_key)",
            "CREATE INDEX bounded_tie_orders_key_idx \
             ON bounded_tie_orders USING btree (order_user_key)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        hydrate_cardinality(&cassie, "bounded_tie_users");
        hydrate_cardinality(&cassie, "bounded_tie_orders");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("bounded_tie_users", "bounded_tie_orders", 2),
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            metrics["read_paths"]["last_index_scan_collection"].as_str(),
            Some("bounded_tie_users")
        );
        assert_eq!(
            metrics["read_paths"]["last_index_scan_index"].as_str(),
            Some("bounded_tie_users_key_idx")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_select_smaller_build_side_for_late_match_bounded_inner_join() {
    // Arrange
    with_fallback();
    let path = data_dir("late_match_bounded_inner_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(&cassie, &session, "late_small_users", "late_large_orders");
        put_users(&cassie, "late_small_users", 2);
        let order_keys = std::iter::repeat_n(99_i64, 198)
            .chain([0_i64, 1_i64])
            .collect::<Vec<_>>();
        put_orders(&cassie, "late_large_orders", &order_keys);
        hydrate_cardinality(&cassie, "late_small_users");
        hydrate_cardinality(&cassie, "late_large_orders");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("late_small_users", "late_large_orders", 2),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(0)],
                vec![Value::String("user-001".to_string()), Value::Int64(1)],
            ]
        );
        assert!(metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]) <= 2);
        assert_eq!(
            after["read_paths"]["last_collection_scan_collection"].as_str(),
            Some("late_large_orders")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_existing_streaming_join_when_cardinality_stats_are_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("missing_stats_bounded_inner_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(
            &cassie,
            &session,
            "missing_stats_users",
            "missing_stats_orders",
        );
        put_users(&cassie, "missing_stats_users", 2);
        let order_keys = std::iter::repeat_n(99_i64, 198)
            .chain([0_i64, 1_i64])
            .collect::<Vec<_>>();
        put_orders(&cassie, "missing_stats_orders", &order_keys);
        cassie
            .catalog
            .clear_cardinality_stats("missing_stats_users");
        cassie
            .catalog
            .clear_cardinality_stats("missing_stats_orders");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                &select_inner_sql("missing_stats_users", "missing_stats_orders", 2),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            metric_delta(&after, &before, &["joins", "vectorized_build_rows_total"]),
            200
        );
        assert_eq!(
            after["read_paths"]["last_collection_scan_collection"].as_str(),
            Some("missing_stats_users")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_apply_bounded_side_selection_to_left_join_or_ordered_join() {
    // Arrange
    with_fallback();
    let path = data_dir("bounded_selection_exclusions");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_join_tables(&cassie, &session, "exclude_users", "exclude_orders");
        put_users(&cassie, "exclude_users", 4);
        put_orders(&cassie, "exclude_orders", &[0, 1, 2, 3]);
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX exclude_orders_key_idx \
                 ON exclude_orders USING btree (order_user_key)",
                vec![],
            )
            .unwrap();
        hydrate_cardinality(&cassie, "exclude_users");
        hydrate_cardinality(&cassie, "exclude_orders");
        let before = cassie.metrics();

        // Act
        let left_join = cassie
            .execute_sql(
                &session,
                "SELECT exclude_users.name, exclude_orders.total \
                 FROM exclude_users LEFT JOIN exclude_orders \
                 ON exclude_users.user_key = exclude_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let ordered_join = cassie
            .execute_sql(
                &session,
                "SELECT exclude_users.name, exclude_orders.total \
                 FROM exclude_users JOIN exclude_orders \
                 ON exclude_users.user_key = exclude_orders.order_user_key \
                 ORDER BY exclude_users.name \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(left_join.rows.len(), 2);
        assert_eq!(ordered_join.rows.len(), 2);
        assert_eq!(
            metric_delta(&after, &before, &["read_paths", "index_seek_scans"]),
            0
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
