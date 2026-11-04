//! Catalog cache admission and live/own version authority.

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "CREATE TABLE cache_own (id INT)", Vec::new())
        .expect("own DDL");
    (cassie, session)
}

#[test]
fn should_admit_the_complete_sql_catalog_cache_before_cloning() {
    // Arrange
    let (_cassie, session) = fixture();
    let sizing = controls(1024 * 1024);
    let owner = session
        .command_catalog_snapshot(&sizing)
        .expect("sizing")
        .expect("RR owner");
    let bytes = owner.accounted_bytes();
    session.transaction.lock().catalog_views.sql = None;
    drop(owner);
    let denied = controls(bytes - 1);
    let exact = controls(bytes);

    // Act
    let failure = session.command_catalog_snapshot(&denied);
    let retained = session
        .command_catalog_snapshot(&exact)
        .expect("exact admission")
        .expect("owner");
    session.rollback_transaction();
    let after_retirement = exact.current_query_memory_bytes();
    drop(retained);

    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(sizing.current_query_memory_bytes(), 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(after_retirement, bytes);
    assert_eq!(exact.current_query_memory_bytes(), 0);
}

#[test]
fn should_refresh_descriptors_for_a_new_main_catalog_version() {
    // Arrange
    let (cassie, session) = fixture();
    let current = controls(1024 * 1024);
    let sql = session
        .command_catalog_snapshot(&current)
        .expect("old plus own")
        .expect("RR owner");
    let previous = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &current)
        .expect("first descriptors")
        .expect("owner");
    let overlay_version = session.transaction_catalog_version();
    let writer = cassie.create_session("tester", None);
    cassie
        .execute_sql(&writer, "CREATE TABLE cache_outside (id INT)", Vec::new())
        .expect("outside DDL");

    // Act
    let refreshed = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &current)
        .expect("live plus own")
        .expect("owner");
    let repeated = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &current)
        .expect("reuse")
        .expect("owner");

    // Assert
    assert_eq!(session.transaction_catalog_version(), overlay_version);
    assert!(!Arc::ptr_eq(&previous, &refreshed));
    assert!(Arc::ptr_eq(&refreshed, &repeated));
    assert!(refreshed.catalog().get_schema("cache_outside").is_some());
    assert!(refreshed.catalog().get_schema("cache_own").is_some());
    assert!(sql.catalog().get_schema("cache_outside").is_none());
    assert!(sql.catalog().get_schema("cache_own").is_some());
}
