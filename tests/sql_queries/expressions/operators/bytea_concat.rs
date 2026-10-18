//! BYTEA overload selection retains type identity across shared string carriers.
use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config};
use cassie::app::CassieError;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

#[test]
fn should_resolve_bytea_unknown_inputs_given_direct_queries_when_concat_overloads_are_selected() {
    // Arrange
    let fixture = sql_fixture("bytea-concat-direct", &[]);
    let cases = [
        (
            "SELECT '\\x00ab'::BYTEA || '\\xff'::BYTEA",
            17,
            Some("\\x00abff"),
        ),
        ("SELECT '\\x00ab'::BYTEA || '\\xff'", 17, Some("\\x00abff")),
        ("SELECT '\\x00ab' || '\\xff'::BYTEA", 17, Some("\\x00abff")),
        ("SELECT '\\x00ab'::BYTEA || NULL", 17, None),
        ("SELECT NULL || '\\xff'::BYTEA", 17, None),
        ("SELECT '\\x00ab'::BYTEA || 't'::TEXT", 25, Some("\\x00abt")),
        ("SELECT 't'::TEXT || '\\x00ab'::BYTEA", 25, Some("t\\x00ab")),
    ];
    // Act
    let observations = cases.map(|(sql, oid, value)| (fixture.execute(sql), oid, value));
    // Assert
    for (result, oid, value) in observations {
        let result = result.expect("native concatenation overload");
        assert_eq!(result.columns()[0].type_oid, oid);
        assert_eq!(
            result.rows(),
            &[vec![
                value.map_or(Value::Null, |value| Value::String(value.into()))
            ]]
        );
    }
}

#[test]
fn should_preserve_bytea_results_given_scopes_and_mutations_when_concat_values_are_reused() {
    // Arrange
    let fixture = sql_fixture(
        "bytea-concat-scopes",
        &[
            "CREATE TABLE records (a BYTEA,b BYTEA)",
            "INSERT INTO records VALUES ('\\x00ab','\\xff')",
            "CREATE TABLE outputs (payload BYTEA)",
        ],
    );
    let statements = [
        "SELECT a || b FROM records",
        "WITH q AS (SELECT a || b AS v FROM records) SELECT v || '\\x'::BYTEA FROM q",
        "SELECT v || '\\x'::BYTEA FROM (SELECT a || b AS v FROM records) AS q",
        "INSERT INTO outputs SELECT a || b FROM records RETURNING payload",
        "UPDATE outputs SET payload=payload || '\\x01'::BYTEA RETURNING payload",
    ];
    // Act
    let results = statements.map(|sql| fixture.execute(sql));
    // Assert
    for (index, result) in results.into_iter().enumerate() {
        let result = result.expect("scoped BYTEA result");
        assert_eq!(result.columns()[0].type_oid, 17);
        assert_eq!(
            result.rows(),
            &[vec![Value::String(
                if index == 4 {
                    "\\x00abff01"
                } else {
                    "\\x00abff"
                }
                .into()
            )]]
        );
    }
}

#[test]
fn should_release_bytea_result_given_disabled_result_cache_when_final_owner_is_dropped() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let fixture = sql_fixture_with_config("bytea-concat-owner", &[], config);
    // Act
    let result = fixture
        .execute("SELECT '\\x00ab'::BYTEA || '\\xff'::BYTEA")
        .expect("BYTEA result");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::String("\\x00abff".into())]]);
    assert!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    drop(result);
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_reject_atomic_bytea_results_given_row_budget_when_concat_outputs_exceed_admission() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_result_rows = 1;
    let fixture = sql_fixture_with_config(
        "bytea-concat-admission",
        &[
            "CREATE TABLE records (a BYTEA,b BYTEA)",
            "INSERT INTO records VALUES ('\\x00','\\xff'), ('\\x01','\\x02')",
        ],
        config,
    );
    // Act
    let result = fixture.execute("SELECT a || b FROM records");
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_observe_cancellation_given_bytea_concat_when_execution_is_cancelled_before_evaluation() {
    // Arrange
    let fixture = sql_fixture("bytea-concat-cancel", &[]);
    let cancellation = cassie::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    // Act
    let result = fixture.cassie.execute_sql_with_cancellation(
        &fixture.session,
        "SELECT '\\x00ab'::BYTEA || '\\xff'::BYTEA",
        vec![],
        &cancellation,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_preserve_known_text_origin_given_computed_operands_when_bytea_concat_is_bound_directly() {
    // Arrange
    let fixture = sql_fixture("bytea-text-origin-direct", &[]);
    let statements = [
        "SELECT '\\x00'::BYTEA || COALESCE('\\xg1') AS v",
        "SELECT '\\x00'::BYTEA || CASE WHEN TRUE THEN '\\xg1' ELSE NULL END AS v",
        "SELECT '\\x00'::BYTEA || NULLIF('\\xg1','other') AS v",
        "SELECT '\\x00'::BYTEA || GREATEST('\\xg1','') AS v",
        "SELECT '\\x00'::BYTEA || CAST('\\xg1' AS VARCHAR(8)) AS v",
        "WITH q AS (SELECT '\\xg1'::TEXT AS v) SELECT '\\x00'::BYTEA || v AS v FROM q",
        "SELECT '\\x00'::BYTEA || v AS v FROM (VALUES ('\\xg1'::TEXT)) AS q(v)",
        "WITH q AS (SELECT COALESCE('\\xg1') AS v) SELECT v || '\\x00'::BYTEA AS v FROM q",
    ];
    // Act
    let results = statements.map(|sql| fixture.execute(sql));
    // Assert
    for (index, result) in results.into_iter().enumerate() {
        let result = result.expect("computed text must retain its known domain");
        assert_eq!(result.columns()[0].type_oid, 25);
        let expected = if index == 7 {
            "\\xg1\\x00"
        } else {
            "\\x00\\xg1"
        };
        assert_eq!(result.rows(), &[vec![Value::String(expected.into())]]);
    }
}
