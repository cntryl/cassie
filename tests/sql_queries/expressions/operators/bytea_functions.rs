//! Fixed BYTEA results keep logical identity through direct queries and mutations.
use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

#[test]
fn should_keep_bytea_identity_given_decode_in_scopes_when_results_are_reused() {
    // Arrange
    let fixture = sql_fixture(
        "bytea-functions-scopes",
        &["CREATE TABLE outputs (payload BYTEA)"],
    );
    let queries = [
        "SELECT decode('00ff','hex')",
        "WITH q AS (SELECT decode('00ff','hex') AS v) SELECT v FROM q",
        "SELECT v FROM (SELECT decode('00ff','hex') AS v) AS q",
        "INSERT INTO outputs SELECT decode('00ff','hex') RETURNING payload",
        "UPDATE outputs SET payload=set_byte(payload,1,-1) RETURNING payload",
    ];
    // Act
    let results = queries.map(|sql| fixture.execute(sql));
    // Assert
    for result in results {
        let result = result.unwrap();
        assert_eq!(result.columns()[0].type_oid, 17);
        assert_eq!(result.rows(), &[vec![Value::String("\\x00ff".into())]]);
    }
}

#[test]
fn should_release_bytea_function_result_given_disabled_cache_when_last_owner_is_dropped() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let fixture = sql_fixture_with_config("bytea-functions-owner", &[], config);
    // Act
    let result = fixture.execute("SELECT decode('00ff','hex')").unwrap();
    // Assert
    assert_eq!(result.rows(), &[vec![Value::String("\\x00ff".into())]]);
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
fn should_reject_text_carrier_given_empty_table_when_decode_result_is_sent_to_lower() {
    // Arrange
    let fixture = sql_fixture(
        "bytea-functions-empty-direct",
        &["CREATE TABLE inputs (value TEXT)"],
    );
    // Act
    let result = fixture.execute("SELECT lower(decode(value,'hex')) FROM inputs");
    // Assert
    assert_eq!(
        result.unwrap_err().to_string(),
        "execution error: function lower(bytea) does not exist"
    );
}
