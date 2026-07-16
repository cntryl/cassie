use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-joins-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn vectorized_join_config(batch_size: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = batch_size;
    config
}

#[test]
fn should_record_merge_join_runtime_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("merge_join_runtime_metrics");
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
            selected.rows,
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
    with_fallback();
    let path = data_dir("vectorized_join_runtime_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, vectorized_join_config(1)).unwrap();
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
            selected.rows,
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
        assert_eq!(metrics["joins"]["last_strategy"], "vectorized");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_vectorized_join_spill_fallback() {
    // Arrange
    with_fallback();
    let path = data_dir("vectorized_join_spill_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config(2);
        config.limits.query_memory_budget_bytes = 800;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_spill_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_spill_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_spill_users (user_key, name) VALUES (1, 'ada')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_spill_orders (order_user_key, total) VALUES (2, 42)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_spill_users.name, metrics_spill_orders.total FROM metrics_spill_users JOIN metrics_spill_orders ON metrics_spill_users.user_key = metrics_spill_orders.order_user_key",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());
        let metrics = cassie.metrics();
        assert_eq!(metrics["joins"]["vectorized_joins"], 0);
        assert_eq!(metrics["joins"]["vectorized_fallbacks"], 1);
        assert_eq!(metrics["joins"]["vectorized_spill_fallbacks"], 1);
        assert_eq!(
            metrics["joins"]["last_vectorized_fallback_reason"],
            "spill_budget_exceeded"
        );
        assert_eq!(metrics["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
}
