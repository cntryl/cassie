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

fn vectorized_join_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config
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
        assert!(plan.contains("projection_shape=runtime_join_degraded"));

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
        assert!(plan.contains("early_stop=exists"));

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
        assert!(plan.contains("early_stop=exists"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_merge_join_strategy_when_ordering_matches_equi_key() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_merge_join");
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
                "CREATE TABLE sql_merge_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_merge_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_merge_join_users.name, sql_merge_join_orders.total FROM sql_merge_join_users JOIN sql_merge_join_orders ON sql_merge_join_users.user_key = sql_merge_join_orders.order_user_key ORDER BY sql_merge_join_users.user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=merge"));
        assert!(plan.contains("join_keys=sql_merge_join_users.user_key=sql_merge_join_orders.order_user_key"));
        assert!(plan.contains("join_sort_required=true"));
        assert!(plan.contains("join_fallback_reason=none"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_select_merge_join_for_non_equi_predicate() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_non_equi_join_fallback");
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
                "CREATE TABLE sql_non_equi_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_non_equi_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_non_equi_join_users.name, sql_non_equi_join_orders.total FROM sql_non_equi_join_users JOIN sql_non_equi_join_orders ON sql_non_equi_join_users.user_key > sql_non_equi_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("join_strategy=nested_loop"));
        assert!(plan.contains("join_fallback_reason=non_equi_predicate"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_vectorized_join_enabled_for_inner_equi_join() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_vectorized_join_enabled");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_vector_join_users.name, sql_vector_join_orders.total FROM sql_vector_join_users JOIN sql_vector_join_orders ON sql_vector_join_users.user_key = sql_vector_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("vectorized_join_candidate=true"));
        assert!(plan.contains("vectorized_join_enabled=true"));
        assert!(plan.contains("vectorized_join_batch_size=2"));
        assert!(plan.contains("vectorized_join_fallback_reason=none"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_vectorized_join_fallback_for_unsupported_join_type() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_vectorized_join_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config()).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_full_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_full_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT sql_vector_full_users.name, sql_vector_full_orders.total FROM sql_vector_full_users FULL OUTER JOIN sql_vector_full_orders ON sql_vector_full_users.user_key = sql_vector_full_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("vectorized_join_candidate=false"));
        assert!(plan.contains("vectorized_join_enabled=false"));
        assert!(plan.contains("vectorized_join_fallback_reason=unsupported_join_type"));

        let _ = std::fs::remove_dir_all(path);
    });
}
