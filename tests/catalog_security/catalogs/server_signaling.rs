//! Session-role boundaries and selected physical server signaling rejection.

use crate::support_backend_sessions::assert_rows;
use crate::support_catalog_policy as policy;
use crate::support_pgwire as wire;

#[test]
fn should_reject_selected_server_signaling_helpers() {
    // Arrange
    let statements = [
        "CREATE ROLE signal_policy_limited",
        "SELECT to_regprocedure('pg_catalog.pg_reload_conf()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_rotate_logfile()') IS NOT NULL AND to_regprocedure('pg_catalog.pg_log_backend_memory_contexts(integer)') IS NOT NULL",
        "SET ROLE signal_policy_limited",
        "SELECT pg_reload_conf()",
        "SELECT pg_rotate_logfile()",
        "SELECT pg_log_backend_memory_contexts(0)",
        "SELECT 42",
        "DROP ROLE signal_policy_limited",
    ];

    // Act
    let batches = policy::run("server-signaling-permissions-policy", &statements);

    // Assert
    assert_rows(&batches[0], &[]);
    policy::assert_registered(&batches[1]);
    policy::assert_error(&batches[2], "0A000");
    for (function, frames) in [
        ("pg_reload_conf", &batches[3]),
        ("pg_rotate_logfile", &batches[4]),
        ("pg_log_backend_memory_contexts", &batches[5]),
    ] {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("0A000"),
            "server helper {function} must reject with the selected feature SQLSTATE"
        );
    }
    assert_rows(&batches[6], &[&["42"]]);
    assert_rows(&batches[7], &[]);
}
