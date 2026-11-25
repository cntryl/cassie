use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use super::support_metrics as support;
use support::*;

fn vectorized_join_config(batch_size: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = batch_size;
    config
}

#[test]
fn should_record_merge_join_runtime_metrics() {
    // Arrange
    let path = data_dir("merge_join_runtime_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_join_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_join_orders (order_user_key, total) VALUES (1, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_join_users.name, metrics_join_orders.total FROM metrics_join_users JOIN metrics_join_orders ON metrics_join_users.user_key = metrics_join_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("ada".to_string()), Value::Int64(42)]]
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["executions"], 1);
        assert_eq!(metrics["joins"]["merge_joins"], 1);
        assert_eq!(metrics["joins"]["matched_rows_total"], 1);
        assert_eq!(metrics["joins"]["output_rows_total"], 1);
        assert_eq!(metrics["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_vectorized_join_runtime_metrics() {
    // Arrange
    let path = data_dir("vectorized_join_runtime_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_in_memory_with_config(vectorized_join_config(1)).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_vector_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_vector_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO metrics_vector_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO metrics_vector_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO metrics_vector_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO metrics_vector_orders (order_user_key, total) VALUES (2, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_vector_users.name, metrics_vector_orders.total FROM metrics_vector_users JOIN metrics_vector_orders ON metrics_vector_users.user_key = metrics_vector_orders.order_user_key ORDER BY metrics_vector_users.name",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(7)]
            ]
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["vectorized_joins"], 1);
        assert_eq!(metrics["joins"]["vectorized_batches_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_build_rows_total"], 2);
        assert_eq!(metrics["joins"]["vectorized_probe_rows_total"], 2);
        assert_eq!(metrics["joins"]["last_strategy"], "typed_hash");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_loaded_join_inputs_before_operator_selection() {
    // Arrange
    let path = data_dir("vectorized_join_spill_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config(2);
        config.limits.query_memory_budget_bytes = 1_023;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        // Literal sources isolate the join strategy from storage-tree decode reservations.
        // Each literal scan fits independently. Combined qualified input admission
        // exceeds this cap before selecting an algorithm; operator spill diagnostics
        // are qualified separately by the actual loaded-kernel unit fixture.
        for input_sql in ["SELECT 1 AS a", "SELECT 2 AS b"] {
            let input = cassie
                .execute_sql(&session, input_sql, vec![])
                .expect("the complete spill input must fit");
            assert_eq!(input.rows().len(), 1);
        }

        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT u.a FROM (SELECT 1 AS a) AS u JOIN (SELECT 2 AS b) AS v ON u.a = v.b",
                vec![],
            )
            .expect_err("qualified loaded inputs must reject before operator selection");

        // Assert
        assert!(
            matches!(error, cassie::app::CassieError::ResourceLimit(_)),
            "the loaded input admission must fail before operator selection: {error:?}"
        );
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["vectorized_joins"], 0);
        assert_eq!(metrics["joins"]["vectorized_fallbacks"], 0);
        assert_eq!(metrics["joins"]["vectorized_spill_fallbacks"], 0);
        assert_eq!(
            metrics["joins"]["last_vectorized_fallback_reason"],
            before["joins"]["last_vectorized_fallback_reason"]
        );
        assert_eq!(metrics["joins"]["executions"], 0);
        assert_eq!(
            metrics["query"]["rows_returned_total"],
            before["query"]["rows_returned_total"]
        );
        assert_eq!(metrics["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(metrics["runtime"]["running_queries"], 0);
        assert_eq!(metrics["runtime"]["active_operator_workers"], 0);

        let _ = std::fs::remove_dir_all(path);
    });
}
