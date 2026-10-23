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
fn should_release_query_memory_after_repeat_output_is_rejected() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 4096;
    let fixture = sql_fixture_with_config("issue-1034-repeat-budget", &[], config);
    let memory_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute("SELECT repeat('x', 4096)");
    let memory_after = current_accounted_memory_bytes(&fixture);
    let follow_up = fixture.execute("SELECT 45::BIGINT");

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(memory_after, memory_before);
    assert_eq!(
        follow_up
            .expect("query after rejected repeat expansion")
            .rows(),
        &[vec![Value::Int64(45)]]
    );
}

#[test]
fn should_release_query_memory_after_padding_output_is_rejected() {
    // Arrange
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 4096;
    let fixture = sql_fixture_with_config("issue-1034-padding-budget", &[], config);
    let memory_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute("SELECT lpad('x', 4096, '🙂')");
    let memory_after = current_accounted_memory_bytes(&fixture);
    let follow_up = fixture.execute("SELECT 45::BIGINT");

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(memory_after, memory_before);
    assert_eq!(
        follow_up
            .expect("query after rejected padding expansion")
            .rows(),
        &[vec![Value::Int64(45)]]
    );
}
