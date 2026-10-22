use super::support_executor as support;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use support::*;

#[test]
fn should_stop_collection_scan_after_limit_is_satisfied() {
    // Arrange
    let path = data_dir("streaming-limit");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE streaming_limit (id TEXT, payload TEXT)",
            vec![],
        )
        .expect("create table");
    for index in 0..50 {
        cassie
            .midge
            .put_document(
                "streaming_limit",
                Some(format!("doc-{index:02}")),
                serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
            )
            .expect("seed row");
    }
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM streaming_limit LIMIT 1",
            vec![],
        )
        .expect("bounded query");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("value".to_string())]]
    );
    assert_eq!(visited, 1, "LIMIT 1 should visit exactly one stored row");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stop_collection_scan_when_result_row_cap_is_exceeded() {
    // Arrange
    let path = data_dir("streaming-result-cap");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE streaming_result_cap (id TEXT, payload TEXT)",
            vec![],
        )
        .expect("create table");
    for index in 0..50 {
        cassie
            .midge
            .put_document(
                "streaming_result_cap",
                Some(format!("doc-{index:02}")),
                serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
            )
            .expect("seed row");
    }
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let error = cassie
        .execute_sql(&session, "SELECT payload FROM streaming_result_cap", vec![])
        .expect_err("result cap should reject the second row");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert_eq!(visited, 2, "row cap should stop after the first excess row");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stop_collection_scan_after_exists_finds_a_row() {
    // Arrange
    let path = data_dir("streaming-exists");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE streaming_exists (id TEXT, payload TEXT)",
            vec![],
        )
        .expect("create inner table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE streaming_exists_outer (id TEXT, payload TEXT)",
            vec![],
        )
        .expect("create outer table");
    cassie
        .midge
        .put_document(
            "streaming_exists_outer",
            Some("outer".to_string()),
            serde_json::json!({"id": "outer", "payload": "outer"}),
        )
        .expect("seed outer row");
    for index in 0..50 {
        cassie
            .midge
            .put_document(
                "streaming_exists",
                Some(format!("doc-{index:02}")),
                serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
            )
            .expect("seed row");
    }
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM streaming_exists_outer WHERE EXISTS(SELECT payload FROM streaming_exists)",
            vec![],
        )
        .expect("exists query");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("outer".to_string())]]
    );
    assert_eq!(visited, 2, "EXISTS should stop after its first inner row");

    let _ = std::fs::remove_dir_all(path);
}
