use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::runtime::QueryCancellationHandle;
use uuid::Uuid;

fn configured_cassie(label: &str) -> (Cassie, String) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = std::env::temp_dir()
        .join(format!(
            "cassie-rest-cancellation-{label}-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .into_owned();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password = "postgres".to_string();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");
    (cassie, path)
}

fn seed_rows(cassie: &Cassie, table: &str, count: usize) {
    let rows = (0..count)
        .map(|index| {
            (
                Some(format!("doc-{index:04}")),
                serde_json::json!({"payload": format!("value-{index:04}")}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, rows)
        .expect("seed rows");
}

#[test]
fn should_propagate_acknowledged_rest_read_cancellation_without_leaking_resources() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("read");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_cancelled_read (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_rows(&cassie, "rest_cancelled_read", 16);
    set_query_scan_cancellation_after_entries(Some(3));
    let body = br#"{"sql":"SELECT payload FROM rest_cancelled_read"}"#;

    // Act
    let result = cassie::rest::query::execute_with_session_and_cancellation(
        &cassie,
        &session,
        body,
        &QueryCancellationHandle::new(),
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("controlled read must acknowledge cancellation"),
    };
    let metrics = cassie.metrics();

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_zero_writes_when_rest_mutation_is_cancelled_before_commit() {
    // Arrange
    let (cassie, path) = configured_cassie("mutation");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_cancelled_mutation (value BIGINT UNIQUE)",
            vec![],
        )
        .expect("create table");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let body = br#"{"sql":"INSERT INTO rest_cancelled_mutation (value) VALUES (1), (2), (3)"}"#;

    // Act
    let result = cassie::rest::query::execute_with_session_and_cancellation(
        &cassie,
        &session,
        body,
        &cancellation,
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("cancelled mutation must not publish"),
    };
    let result = cassie
        .execute_sql(
            &session,
            "SELECT COUNT(*) AS count FROM rest_cancelled_mutation",
            vec![],
        )
        .expect("count rows");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(0)]]);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_zero_documents_when_rest_document_write_is_cancelled_before_publication() {
    // Arrange
    let (cassie, path) = configured_cassie("document-mutation");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_cancelled_document (payload TEXT)",
            vec![],
        )
        .expect("create table");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let result = cassie::rest::documents::create_with_cancellation(
        &cassie,
        "rest_cancelled_document",
        br#"{"payload":"must-not-commit"}"#,
        &cancellation,
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("cancelled REST document write must not publish"),
    };
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT COUNT(*) AS count FROM rest_cancelled_document",
            vec![],
        )
        .expect("count documents");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(rows.rows, vec![vec![cassie::types::Value::Int64(0)]]);

    let _ = std::fs::remove_dir_all(path);
}
