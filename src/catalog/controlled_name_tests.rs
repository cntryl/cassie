use super::name_matches_controlled;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[test]
fn should_match_existing_identifier_suffix_with_quote_behavior() {
    // Arrange
    let cases = [
        ("postgres.public.orders", "orders"),
        ("postgres.public.orders", "PUBLIC.ORDERS"),
        ("db.\"quoted.dot\".\"A\"\"b\"", "\"quoted.dot\".\"a\"\"B\""),
        ("db.public.orders", "other.orders"),
        ("bad..name", "bad..NAME"),
        ("bad..name", "name"),
        ("db.\"unterminated", "unterminated"),
        ("db.public.name", "db.public.name.extra"),
        ("  db . public . orders  ", "orders"),
    ];
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act / Assert
    for (stored, requested) in cases {
        // Assert
        assert_eq!(
            name_matches_controlled(stored, requested, &controls).expect("lookup"),
            crate::catalog::name_matches(stored, requested),
            "{stored:?} {requested:?}"
        );
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_identifier_parse_when_query_budget_is_exhausted() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = name_matches_controlled("public.orders", "orders", &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_stop_controlled_identifier_matching_when_cancelled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    // Act
    let result = name_matches_controlled("public.orders", "orders", &controls);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_controlled_namespace_lookup_against_catalog_behavior() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_namespace("postgres.\"Odd.Schema\"", None);
    catalog.register_namespace("postgres.public", None);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act / Assert
    for name in [
        "postgres.public",
        "PUBLIC",
        "\"odd.schema\"",
        "missing",
        "bad..schema",
    ] {
        // Assert
        assert_eq!(
            catalog
                .namespace_exists_controlled(name, &controls)
                .expect("namespace"),
            catalog.namespace_exists(name)
        );
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
