//! Admission limits and failure atomicity of the record-returning sources, over the embedded API.

use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

const MEMORY_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const RESULT_ROW_LIMIT: usize = 3;

fn configured_cassie(memory_budget: usize, max_result_rows: usize) -> Cassie {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.max_result_rows = max_result_rows;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("configured cassie");
    cassie.startup().expect("startup");
    cassie
}

fn array_of(count: usize) -> String {
    format!("[{}]", vec!["{\"a\":0}"; count].join(","))
}

fn recordset(count: usize) -> String {
    format!(
        "SELECT a FROM jsonb_to_recordset('{}') AS x(a int)",
        array_of(count)
    )
}

fn table(count: usize) -> String {
    format!(
        "SELECT a FROM JSON_TABLE('{}'::jsonb, '$[*]' COLUMNS (a int PATH '$.a'))",
        array_of(count)
    )
}

#[test]
fn should_admit_exactly_the_result_row_limit_given_to_recordset() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, RESULT_ROW_LIMIT);
    let session = cassie.create_session("tester", None);
    // Act
    let admitted = cassie.execute_sql(&session, &recordset(RESULT_ROW_LIMIT), vec![]);
    // Assert
    assert_eq!(
        admitted.expect("recordset at the row limit").rows().len(),
        3
    );
}

#[test]
fn should_reject_one_row_above_the_result_row_limit_given_to_recordset() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, RESULT_ROW_LIMIT);
    let session = cassie.create_session("tester", None);
    // Act
    let rejected = cassie.execute_sql(&session, &recordset(RESULT_ROW_LIMIT + 1), vec![]);
    // Assert
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_reject_one_row_above_the_result_row_limit_given_json_table() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, RESULT_ROW_LIMIT);
    let session = cassie.create_session("tester", None);
    // Act
    let admitted = cassie.execute_sql(&session, &table(RESULT_ROW_LIMIT), vec![]);
    let rejected = cassie.execute_sql(&session, &table(RESULT_ROW_LIMIT + 1), vec![]);
    // Assert
    assert_eq!(admitted.expect("table at the row limit").rows().len(), 3);
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_reject_an_oversized_recordset_given_a_four_mib_memory_budget() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();
    // Act
    let rejected = cassie.execute_sql(&session, &recordset(100_000), vec![]);
    // Assert
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    let after = cassie.metrics();
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(
        after["query"]["rows_returned_total"],
        before["query"]["rows_returned_total"]
    );
}

#[test]
fn should_reject_an_oversized_json_table_given_a_four_mib_memory_budget() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    // Act
    let rejected = cassie.execute_sql(&session, &table(100_000), vec![]);
    // Assert
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_release_memory_given_a_conversion_error_after_admission() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();
    // Act
    let rejected = cassie.execute_sql(
        &session,
        "SELECT a FROM jsonb_to_recordset('[{\"a\":1},{\"a\":\"x\"}]') AS x(a int)",
        vec![],
    );
    // Assert
    assert!(rejected.is_err());
    let after = cassie.metrics();
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(
        after["query"]["rows_returned_total"],
        before["query"]["rows_returned_total"]
    );
}

#[test]
fn should_release_memory_given_an_error_in_a_later_json_table_column() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    // Act
    let rejected = cassie.execute_sql(
        &session,
        "SELECT a FROM JSON_TABLE('[{\"a\":1},{\"a\":[1,2]}]'::jsonb, '$[*]' \
         COLUMNS (a int PATH '$.a' ERROR ON ERROR))",
        vec![],
    );
    // Assert
    assert!(rejected.is_err());
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}
