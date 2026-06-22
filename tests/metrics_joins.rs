use cassie::app::Cassie;
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
