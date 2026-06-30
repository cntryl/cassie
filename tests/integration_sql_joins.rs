#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig, OperatorSwitchingEnabled,
};
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

fn vectorized_join_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config
}

fn operator_switch_join_config(enabled: bool, threshold: usize) -> CassieRuntimeConfig {
    let mut config = vectorized_join_config();
    config.limits.operator_switching_enabled = if enabled {
        OperatorSwitchingEnabled::enabled()
    } else {
        OperatorSwitchingEnabled::disabled()
    };
    config.limits.operator_switch_join_row_threshold = threshold;
    config
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
fn should_execute_merge_join_duplicate_null_keys() {
    // Arrange
    with_fallback();
    let path = data_dir("join_merge_duplicate_null_keys");
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
                "CREATE TABLE merge_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE merge_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO merge_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO merge_users (user_key, name) VALUES (1, 'ada-alt')",
            "INSERT INTO merge_users (user_key, name) VALUES (NULL, 'unknown')",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (1, 99)",
            "INSERT INTO merge_orders (order_user_key, total) VALUES (NULL, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT merge_users.name, merge_orders.total FROM merge_users JOIN merge_orders ON merge_users.user_key = merge_orders.order_user_key ORDER BY merge_users.name, merge_orders.total",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(42)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(99)],
                vec![Value::String("unknown".to_string()), Value::Int64(7)]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "merge");
        assert_eq!(metrics["joins"]["merge_joins"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_vectorized_inner_join_duplicate_null_keys() {
    // Arrange
    with_fallback();
    let path = data_dir("join_vectorized_inner_duplicate_null_keys");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO vector_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO vector_users (user_key, name) VALUES (1, 'ada-alt')",
            "INSERT INTO vector_users (user_key, name) VALUES (NULL, 'unknown')",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (1, 99)",
            "INSERT INTO vector_orders (order_user_key, total) VALUES (NULL, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT vector_users.name, vector_orders.total FROM vector_users JOIN vector_orders ON vector_users.user_key = vector_orders.order_user_key ORDER BY vector_users.name, vector_orders.total",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("ada".to_string()), Value::Int64(99)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(42)],
                vec![Value::String("ada-alt".to_string()), Value::Int64(99)],
                vec![Value::String("unknown".to_string()), Value::Int64(7)]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");
        assert_eq!(metrics["joins"]["vectorized_joins"], 1);
        assert_eq!(metrics["joins"]["last_vectorized_batch_size"], 2);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_vectorized_left_join_unmatched_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("join_vectorized_left_unmatched");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_left_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE vector_left_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO vector_left_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO vector_left_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO vector_left_orders (order_user_key, total) VALUES (1, 42)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT vector_left_users.name, vector_left_orders.total FROM vector_left_users LEFT JOIN vector_left_orders ON vector_left_users.user_key = vector_left_orders.order_user_key ORDER BY vector_left_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Null]
            ]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");
        assert_eq!(metrics["joins"]["vectorized_probe_rows_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_build_rows_total"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_join_results_when_operator_switch_replays_inputs() {
    // Arrange
    with_fallback();
    let fixed_path = data_dir("join_operator_switch_fixed");
    let switched_path = data_dir("join_operator_switch_replay");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let fixed =
            Cassie::new_with_data_dir_and_config(&fixed_path, operator_switch_join_config(false, 0))
                .unwrap();
        let switched =
            Cassie::new_with_data_dir_and_config(&switched_path, operator_switch_join_config(true, 1))
                .unwrap();
        let fixed_session = fixed.create_session("tester", None);
        let switched_session = switched.create_session("tester", None);
        for (cassie, session, users, orders) in [
            (
                &fixed,
                &fixed_session,
                "join_switch_fixed_users",
                "join_switch_fixed_orders",
            ),
            (
                &switched,
                &switched_session,
                "join_switch_replay_users",
                "join_switch_replay_orders",
            ),
        ] {
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE TABLE {users} (user_key INT, name TEXT)"),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!("CREATE TABLE {orders} (order_user_key INT, total INT)"),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "INSERT INTO {users} (user_key, name) VALUES (1, 'ada'), (2, 'grace')"
                    ),
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "INSERT INTO {orders} (order_user_key, total) VALUES (1, 10), (2, 20)"
                    ),
                    vec![],
                )
                .unwrap();
        }

        // Act
        let fixed_result = fixed
            .execute_sql(
                &fixed_session,
                "SELECT join_switch_fixed_users.name, join_switch_fixed_orders.total FROM join_switch_fixed_users JOIN join_switch_fixed_orders ON join_switch_fixed_users.user_key = join_switch_fixed_orders.order_user_key ORDER BY join_switch_fixed_users.name",
                vec![],
            )
            .unwrap();
        let switched_result = switched
            .execute_sql(
                &switched_session,
                "SELECT join_switch_replay_users.name, join_switch_replay_orders.total FROM join_switch_replay_users JOIN join_switch_replay_orders ON join_switch_replay_users.user_key = join_switch_replay_orders.order_user_key ORDER BY join_switch_replay_users.name",
                vec![],
            )
            .unwrap();
        let metrics = switched.metrics();

        // Assert
        assert_eq!(fixed_result.rows, switched_result.rows);
        assert_eq!(
            switched_result.rows,
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(10)],
                vec![Value::String("grace".to_string()), Value::Int64(20)],
            ]
        );
        assert_eq!(
            metrics["adaptive_candidates"]["last_operator_switch_state"],
            "replay_left_rows=2;replay_right_rows=2;rows_emitted=0"
        );

        let _ = std::fs::remove_dir_all(fixed_path);
        let _ = std::fs::remove_dir_all(switched_path);
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
