//! Volatile expressions must be evaluated again instead of replayed.
use super::*;
use crate::types::Value;

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("memory fixture");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE volatile_cache_rows (value INT)", vec![])
        .expect("table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO volatile_cache_rows VALUES (1)",
            vec![],
        )
        .expect("row");
    (cassie, session)
}

#[test]
fn should_not_replay_random_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT random() FROM volatile_cache_rows", vec![])
        .expect("first random query");
    let second = cassie
        .execute_sql(&session, "SELECT random() FROM volatile_cache_rows", vec![])
        .expect("second random query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_omitted_volatility_function_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION r() RETURNS float AS "random()""#,
            vec![],
        )
        .expect("volatile-by-default function");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT r() FROM volatile_cache_rows", vec![])
        .expect("first routine query");
    let second = cassie
        .execute_sql(&session, "SELECT r() FROM volatile_cache_rows", vec![])
        .expect("second routine query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_replay_pure_select_results_from_execution_cache() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows",
            vec![],
        )
        .expect("first pure query");
    let second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows",
            vec![],
        )
        .expect("second pure query");

    // Assert
    assert_eq!(first.rows(), vec![vec![Value::Int64(1)]]);
    assert_eq!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 1);
}
