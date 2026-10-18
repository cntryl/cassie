//! Commit tracking defaults and PostgreSQL physical control-data rejection.

use crate::support_backend_sessions::assert_rows;
use crate::support_catalog_policy as policy;

#[test]
fn should_follow_commit_timestamp_policy_given_transaction_outcomes_when_commit_timestamp_helpers_are_called(
) {
    // Arrange
    // The selected default matches PostgreSQL: tracking is off. A typed NULL
    // control proves the function signature before the non-NULL error path.
    let statements = [
        "SELECT current_setting('track_commit_timestamp')",
        "SELECT pg_xact_commit_timestamp(NULL::xid), pg_xact_commit_timestamp_origin(NULL::xid)",
        "SELECT pg_xact_commit_timestamp(pg_current_xact_id()::text::xid)",
        "SELECT * FROM pg_xact_commit_timestamp_origin(pg_current_xact_id()::text::xid)",
        "SELECT * FROM pg_last_committed_xact()",
        "SELECT 42",
    ];

    // Act
    let batches = policy::run("commit-timestamp-disabled-policy", &statements);

    // Assert
    assert_rows(&batches[0], &[&["off"]]);
    policy::assert_nulls(&batches[1], &[1184, 2249]);
    for frames in &batches[2..5] {
        policy::assert_error(frames, "55000");
    }
    assert_rows(&batches[5], &[&["42"]]);
}

#[test]
fn should_follow_control_data_policy_given_an_authenticated_session_when_control_helpers_are_called(
) {
    // Arrange
    // These registered signatures deliberately reject PostgreSQL physical
    // control-file access on the direct-Midge architecture.
    let statements = [
        "SELECT to_regprocedure('pg_catalog.pg_control_checkpoint()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_control_system()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_control_init()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_control_recovery()') IS NOT NULL",
        "SELECT * FROM pg_control_checkpoint()",
        "SELECT * FROM pg_control_system()",
        "SELECT * FROM pg_control_init()",
        "SELECT * FROM pg_control_recovery()",
        "SELECT 42",
    ];

    // Act
    let batches = policy::run("physical-control-data-policy", &statements);

    // Assert
    policy::assert_registered(&batches[0]);
    for frames in &batches[1..5] {
        policy::assert_error(frames, "0A000");
    }
    assert_rows(&batches[5], &[&["42"]]);
}
