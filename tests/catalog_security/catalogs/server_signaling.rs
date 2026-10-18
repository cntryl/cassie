//! Permission checks precede the selected physical server signaling rejection.

use crate::support_backend_sessions::assert_rows;
use crate::support_catalog_policy as policy;

#[test]
fn should_enforce_signal_permissions_given_restricted_roles_when_server_signaling_functions_are_called(
) {
    // Arrange
    let statements = [
        "CREATE ROLE signal_policy_limited",
        "SELECT to_regprocedure('pg_catalog.pg_reload_conf()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_rotate_logfile()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_log_backend_memory_contexts(integer)') IS NOT NULL",
        "SET ROLE signal_policy_limited",
        "SELECT pg_reload_conf()",
        "SELECT pg_rotate_logfile()",
        "SELECT pg_log_backend_memory_contexts(0)",
        "RESET ROLE",
        "SELECT pg_reload_conf()",
        "SELECT pg_rotate_logfile()",
        "SELECT pg_log_backend_memory_contexts(0)",
        "SELECT 42",
    ];

    // Act
    let batches = policy::run("server-signaling-permissions-policy", &statements);

    // Assert
    assert_rows(&batches[0], &[]);
    policy::assert_registered(&batches[1]);
    assert_rows(&batches[2], &[]);
    for frames in &batches[3..6] {
        policy::assert_error(frames, "42501");
    }
    assert_rows(&batches[6], &[]);
    for frames in &batches[7..10] {
        policy::assert_error(frames, "0A000");
    }
    assert_rows(&batches[10], &[&["42"]]);
}
