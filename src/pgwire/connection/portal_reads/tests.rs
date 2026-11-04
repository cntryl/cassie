//! Directory admission follows the actual private portal owner backing.

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

#[test]
fn should_release_the_read_directory_when_its_last_portal_closes() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "SELECT 1", Vec::new())
        .expect("RR owner");
    let query = controls(1024 * 1024);
    let retention = controls(1024 * 1024);
    let frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &query,
            &retention,
        )
        .expect("frame")
        .expect("RR frame");
    let bytes = 4 * size_of::<PortalReadOwner>() + "portal".len();
    let denied = controls(bytes - 1);
    let exact = controls(bytes);
    let mut rejected = PortalReadOwners::default();
    let mut owners = PortalReadOwners::default();

    // Act
    let failure = rejected.insert("portal", Arc::clone(&frame), &denied);
    owners
        .insert("portal", Arc::clone(&frame), &exact)
        .expect("exact directory admission");
    let worker = owners.get("portal").expect("worker clone");
    owners.remove("portal");
    drop(frame);
    let worker_charge = retention.current_query_memory_bytes();
    drop(worker);

    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert!(worker_charge > 0);
    assert_eq!(retention.current_query_memory_bytes(), 0);
    assert_eq!(query.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_a_frame_together_with_its_portal_directory() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "SELECT 1", Vec::new())
        .expect("RR owner");
    let current = controls(1024 * 1024);
    let sizing = controls(1024 * 1024);
    let measured = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &current,
            &sizing,
        )
        .expect("sizing")
        .expect("frame");
    let frame_bytes = sizing.current_query_memory_bytes();
    drop(measured);
    let directory_bytes = 4 * size_of::<PortalReadOwner>() + "portal".len();
    let denied = controls(frame_bytes + directory_bytes - 1);
    let exact = controls(frame_bytes + directory_bytes);
    let mut rejected = PortalReadOwners::default();
    let mut accepted = PortalReadOwners::default();

    // Act
    let denied_frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &current,
            &denied,
        )
        .expect("frame alone fits")
        .expect("frame");
    let failure = rejected.insert("portal", denied_frame, &denied);
    let exact_frame = session
        .capture_bound_read_view(
            &cassie.midge,
            &cassie.default_database,
            false,
            &current,
            &exact,
        )
        .expect("frame fits")
        .expect("frame");
    accepted
        .insert("portal", exact_frame, &exact)
        .expect("frame with directory fits");
    let retained = exact.current_query_memory_bytes();
    accepted.remove("portal");

    // Assert
    assert!(matches!(failure, Err(CassieError::ResourceLimit(_))));
    assert_eq!(sizing.current_query_memory_bytes(), 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(retained, frame_bytes + directory_bytes);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}
