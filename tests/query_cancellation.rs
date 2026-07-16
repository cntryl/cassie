use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("cassie-{label}-{}", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn should_expose_query_memory_budget_with_clean_baseline_name() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();

    // Act
    config.limits.query_memory_budget_bytes = 4_096;

    // Assert
    assert_eq!(config.limits.query_memory_budget_bytes, 4_096);
}

#[test]
fn should_report_cancellation_handle_state() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();

    // Act
    cancellation.cancel();

    // Assert
    assert!(cancellation.is_cancelled());
}

#[test]
fn should_cancel_embedded_query_before_execution() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir("embedded-query-cancelled");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let error = cassie
        .execute_sql_with_cancellation(&session, "SELECT 1", vec![], &cancellation)
        .expect_err("cancelled query should stop before execution");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_query_memory_above_remaining_budget() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 10;
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let reservation = controls.reserve_query_memory(8).expect("first reservation");

    // Act
    let error = controls
        .reserve_query_memory(3)
        .expect_err("combined reservations should exceed the budget");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert_eq!(controls.peak_query_memory_bytes(), 8);
    drop(reservation);
}

#[test]
fn should_release_query_memory_when_reservation_drops() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 10;
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let reservation = controls.reserve_query_memory(10).expect("full reservation");
    drop(reservation);

    // Act
    let replacement = controls
        .reserve_query_memory(10)
        .expect("released bytes should be reusable");

    // Assert
    assert_eq!(controls.peak_query_memory_bytes(), 10);
    drop(replacement);
}

#[test]
fn should_cancel_embedded_query_during_recursive_execution() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir("embedded-query-active-cancel");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.cte_recursion_depth = 1_000_000;
    config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
    let cassie =
        Arc::new(Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie"));
    cassie.startup().expect("startup");
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        let session = query_cassie.create_session("tester", None);
        query_cassie.execute_sql_with_cancellation(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq",
            vec![],
            &query_cancellation,
        )
    });
    std::thread::sleep(Duration::from_millis(25));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("query thread")
        .expect_err("active query should be cancelled");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_embedded_query_during_ordered_execution() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir("embedded-ordered-query-cancel");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
    let cassie =
        Arc::new(Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie"));
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE cancellation_sort_rows (value TEXT)",
            vec![],
        )
        .expect("create table");
    let rows = (0..20_000)
        .map(|index| {
            (
                Some(format!("row-{index:05}")),
                serde_json::json!({"value": format!("value-{:05}", 20_000 - index)}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("cancellation_sort_rows", rows)
        .expect("seed rows");
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_cassie = Arc::clone(&cassie);
    let query = std::thread::spawn(move || {
        let session = query_cassie.create_session("tester", None);
        query_cassie.execute_sql_with_cancellation(
            &session,
            "SELECT value FROM cancellation_sort_rows ORDER BY value",
            vec![],
            &query_cancellation,
        )
    });
    std::thread::sleep(Duration::from_millis(10));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("query thread")
        .expect_err("ordered query should be cancelled");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
