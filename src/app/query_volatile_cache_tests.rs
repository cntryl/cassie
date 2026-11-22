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

#[test]
fn should_not_replay_volatile_results_hidden_in_scalar_subquery_or_exists() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let scalar_first = cassie
        .execute_sql(&session, "SELECT (SELECT random())", vec![])
        .expect("scalar subquery");
    let scalar_second = cassie
        .execute_sql(&session, "SELECT (SELECT random())", vec![])
        .expect("scalar subquery replay");
    let exists_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows WHERE EXISTS (SELECT random())",
            vec![],
        )
        .expect("exists subquery");
    let exists_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows WHERE EXISTS (SELECT random())",
            vec![],
        )
        .expect("exists subquery replay");

    // Assert
    assert_ne!(scalar_first.rows(), scalar_second.rows());
    assert_eq!(exists_first.rows(), exists_second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_view() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_view AS SELECT random() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("view");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("view query");
    let second = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_view", vec![])
        .expect("view replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_in_limit_offset_or_table_sample() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let limit_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT (random() * 0)::int",
            vec![],
        )
        .expect("limit query");
    let limit_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows LIMIT (random() * 0)::int",
            vec![],
        )
        .expect("limit replay");
    let sample_first = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows TABLESAMPLE bernoulli (random() * 100)",
            vec![],
        )
        .expect("table sample query");
    let sample_second = cassie
        .execute_sql(
            &session,
            "SELECT value FROM volatile_cache_rows TABLESAMPLE bernoulli (random() * 100)",
            vec![],
        )
        .expect("table sample replay");

    // Assert
    assert_eq!(limit_first.rows(), limit_second.rows());
    assert_eq!(sample_first.columns(), sample_second.columns());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_cte() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "WITH volatile AS (SELECT random() AS value) SELECT value FROM volatile",
            vec![],
        )
        .expect("volatile CTE");
    let second = cassie
        .execute_sql(
            &session,
            "WITH volatile AS (SELECT random() AS value) SELECT value FROM volatile",
            vec![],
        )
        .expect("volatile CTE replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_volatile_results_hidden_in_a_set_branch() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let first = cassie
        .execute_sql(
            &session,
            "SELECT random() AS value UNION ALL SELECT random()",
            vec![],
        )
        .expect("volatile set branch");
    let second = cassie
        .execute_sql(
            &session,
            "SELECT random() AS value UNION ALL SELECT random()",
            vec![],
        )
        .expect("volatile set branch replay");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_default_volatile_udf_results_hidden_in_a_view() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            r#"CREATE FUNCTION r() RETURNS float AS "random()""#,
            vec![],
        )
        .expect("default volatile function");
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_udf_view AS SELECT r() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("volatile function view");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_udf_view", vec![])
        .expect("first view query");
    let second = cassie
        .execute_sql(&session, "SELECT value FROM volatile_cache_udf_view", vec![])
        .expect("second view query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}

#[test]
fn should_not_replay_random_results_from_a_view_with_catalog_enum_types() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(
            &session,
            "CREATE TYPE volatile_cache_mood AS ENUM ('sad')",
            vec![],
        )
        .expect("enum type");
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW volatile_cache_enum_view AS SELECT 'sad'::volatile_cache_mood AS mood, random() AS value FROM volatile_cache_rows",
            vec![],
        )
        .expect("enum-typed volatile view");

    // Act
    let first = cassie
        .execute_sql(&session, "SELECT mood, value FROM volatile_cache_enum_view", vec![])
        .expect("first enum view query");
    let second = cassie
        .execute_sql(&session, "SELECT mood, value FROM volatile_cache_enum_view", vec![])
        .expect("second enum view query");

    // Assert
    assert_ne!(first.rows(), second.rows());
    assert_eq!(cassie.metrics()["execution_result_cache"]["hits"], 0);
}
