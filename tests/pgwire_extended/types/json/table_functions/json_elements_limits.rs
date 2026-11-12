//! Admission limits and failure atomicity for the JSON element sources, over the embedded API.

use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

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
    format!("[{}]", vec!["0"; count].join(","))
}

#[test]
fn should_admit_a_small_expansion_and_reject_an_oversized_one_given_the_same_memory_budget() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    let small = format!("SELECT value FROM jsonb_array_elements('{}')", array_of(16));
    let oversized = format!(
        "SELECT value FROM jsonb_array_elements('{}')",
        array_of(100_000)
    );
    // Act
    let admitted = cassie.execute_sql(&session, &small, vec![]);
    let after_admitted = cassie.metrics();
    let rejected = cassie.execute_sql(&session, &oversized, vec![]);
    // Assert
    assert_eq!(admitted.expect("small expansion").rows().len(), 16);
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    let after_rejected = cassie.metrics();
    assert_eq!(after_rejected["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(
        after_rejected["query"]["rows_returned_total"],
        after_admitted["query"]["rows_returned_total"],
        "the rejected expansion must not publish rows"
    );
}

#[test]
fn should_admit_the_result_row_limit_and_reject_one_more_row_given_array_elements() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, RESULT_ROW_LIMIT);
    let session = cassie.create_session("tester", None);
    let at_limit = "SELECT value FROM jsonb_array_elements('[1,2,3]'::jsonb)";
    let above_limit = "SELECT value FROM jsonb_array_elements('[1,2,3,4]'::jsonb)";
    // Act
    let admitted = cassie.execute_sql(&session, at_limit, vec![]);
    let rejected = cassie.execute_sql(&session, above_limit, vec![]);
    // Assert
    assert_eq!(
        admitted
            .expect("expansion at the row limit")
            .rows()
            .to_vec(),
        vec![
            vec![Value::Json(serde_json::json!(1))],
            vec![Value::Json(serde_json::json!(2))],
            vec![Value::Json(serde_json::json!(3))],
        ]
    );
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_release_expansion_memory_given_a_wrong_shape_error_after_admission() {
    // Arrange
    let cassie = configured_cassie(MEMORY_BUDGET_BYTES, usize::MAX);
    let session = cassie.create_session("tester", None);
    let before = cassie.metrics();
    // Act
    let rejected = cassie.execute_sql(
        &session,
        "SELECT value FROM jsonb_array_elements('{\"a\":1}'::jsonb)",
        vec![],
    );
    // Assert
    assert!(matches!(
        rejected,
        Err(CassieError::InvalidParameterValue(_))
    ));
    let after = cassie.metrics();
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(
        after["query"]["rows_returned_total"],
        before["query"]["rows_returned_total"]
    );
}
