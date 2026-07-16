#[path = "../benches/support/workloads.rs"]
mod workloads;

use cassie::types::Value;

#[test]
fn should_bind_dynamic_plan_cache_values_given_closed_alias_selection() {
    // Arrange
    let nonce = 17;

    // Act
    let statement = workloads::bound_plan_cache_miss(nonce);

    // Assert
    assert!(statement.sql.contains("id AS miss_17"));
    assert!(statement.sql.contains("score >= $1"));
    assert!(statement.sql.contains("status IN ($2, $3, $4)"));
    assert!(!statement.sql.contains("miss-17"));
    assert_eq!(statement.params[3], Value::String("miss-17".to_string()));
}

#[test]
fn should_cycle_only_closed_plan_cache_identifiers_given_large_nonce() {
    // Arrange
    let first = workloads::bound_plan_cache_miss(0);

    // Act
    let cycled = workloads::bound_plan_cache_miss(64);

    // Assert
    assert_eq!(first.sql, cycled.sql);
    assert_ne!(first.params, cycled.params);
}

#[test]
fn should_bind_recursive_values_given_dynamic_fixture() {
    // Arrange
    let upper_bound = 7;

    // Act
    let recursive = workloads::bound_recursive_cte(upper_bound);

    // Assert
    assert!(recursive.sql.contains("seq.n < $1"));
    assert!(!recursive.sql.contains("seq.n < 7"));
    assert_eq!(recursive.params, [Value::Int64(7)]);
}

#[test]
fn should_bind_time_series_values_given_dynamic_fixture() {
    // Arrange
    let start = "2026-01-10T00:00:00Z";
    let end = "2026-01-12T00:00:00Z";

    // Act
    let time_series = workloads::bound_time_series_window(start, end);

    // Assert
    assert!(time_series.sql.contains("event_at >= $1"));
    assert!(time_series.sql.contains("event_at < $2"));
    assert!(!time_series.sql.contains("2026-01-10"));
    assert_eq!(time_series.params.len(), 2);
}

#[test]
fn should_warm_the_same_bound_fulltext_statement_that_is_measured() {
    // Arrange
    let scaling = include_str!("../benches/support/workloads/scaling.rs");
    let legacy = include_str!("../benches/support/workloads/scaling_legacy.rs");
    let retrieval = include_str!("../benches/tier5_scaling_retrieval.rs");

    // Act
    let production_uses_shared_params = scaling.contains("fulltext_scaling_params()");
    let warmup_uses_shared_params = legacy.contains("super::scaling::fulltext_scaling_params()");
    let preflight_uses_shared_params = retrieval.contains("workloads::fulltext_scaling_params()");

    // Assert
    assert!(production_uses_shared_params);
    assert!(warmup_uses_shared_params);
    assert!(preflight_uses_shared_params);
}
