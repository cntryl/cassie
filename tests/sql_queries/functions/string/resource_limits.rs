use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};

fn current_accounted_memory_bytes(fixture: &SqlFixture) -> u64 {
    fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted query memory")
}

#[test]
fn should_reject_string_to_table_expansion_over_the_result_row_limit() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    let fixture =
        sql_fixture_with_config("issue-803-string-to-table-result-row-limit", &[], config);
    let memory_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute("SELECT string_to_table('a,b,c', ',')");
    let memory_after = current_accounted_memory_bytes(&fixture);
    let follow_up = fixture.execute("SELECT 45::BIGINT");

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "string_to_table expansion beyond max_result_rows must return no partial result and fail as a resource limit: {result:?}"
    );
    assert_eq!(memory_after, memory_before);
    assert_eq!(
        follow_up
            .expect("query after bounded string_to_table expansion")
            .rows(),
        &[vec![Value::Int64(45)]]
    );
}
