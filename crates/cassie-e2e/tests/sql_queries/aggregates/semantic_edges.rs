//! Focused PostgreSQL 18 aggregate edge cases beyond the broader owner suites.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

#[path = "ordered_set/hypothetical_multicolumn.rs"]
mod hypothetical_multicolumn;

fn assert_float_near(value: &Value, expected: f64) {
    let Value::Float64(actual) = value else {
        panic!("expected a float value, got {value:?}");
    };
    assert!(
        (*actual - expected).abs() < 1e-12,
        "expected {expected:?}, got {actual:?}"
    );
}

include!("boolean/aggregate_semantic_edges.rs");

include!("statistical/aggregate_semantic_edges.rs");

include!("ordered_set/aggregate_semantic_edges.rs");

include!("bit_bytea/aggregate_semantic_edges.rs");

include!("windows/aggregate_semantic_edges.rs");

include!("arrays/aggregate_semantic_edges.rs");

include!("json/aggregate_semantic_edges.rs");
