//! Explicit rejection of PostgreSQL backend-statistics resets.

use crate::support_backend_sessions::{assert_rows, with_sessions, Sessions};
use crate::support_pgwire as wire;

async fn assert_backend_ids(sessions: &mut Sessions) {
    let admin = sessions.admin.query("SELECT pg_backend_pid()").await;
    let target = sessions.target.query("SELECT pg_backend_pid()").await;
    assert_rows(&admin, &[&[&sessions.admin.pid.to_string()]]);
    assert_rows(&target, &[&[&sessions.target.pid.to_string()]]);
    assert_ne!(sessions.admin.pid, sessions.target.pid);
}

#[test]
fn should_reject_backend_statistics_reset_explicitly() {
    // Arrange
    with_sessions("backend-statistics-reset-permissions", async |sessions| {
        assert_backend_ids(sessions).await;

        // Act
        let create = sessions
            .admin
            .query("CREATE ROLE backend_stats_limited")
            .await;
        let switch = sessions.admin.query("SET ROLE backend_stats_limited").await;
        let registered = sessions
            .admin
            .query("SELECT to_regprocedure('pg_catalog.pg_stat_reset_backend_stats(integer)') IS NOT NULL")
            .await;
        let self_reset = sessions
            .admin
            .query(&format!(
                "SELECT pg_stat_reset_backend_stats({})",
                sessions.admin.pid
            ))
            .await;
        let other_reset = sessions
            .admin
            .query(&format!(
                "SELECT pg_stat_reset_backend_stats({})",
                sessions.target.pid
            ))
            .await;
        let drop_role = sessions
            .admin
            .query("DROP ROLE backend_stats_limited")
            .await;

        // Assert
        assert_rows(&create, &[]);
        assert_eq!(wire::error_code(&switch).as_deref(), Some("0A000"));
        assert_rows(&registered, &[&["t"]]);
        assert_eq!(wire::error_code(&self_reset).as_deref(), Some("0A000"));
        assert_eq!(wire::error_code(&other_reset).as_deref(), Some("0A000"));
        assert_rows(&drop_role, &[]);
    });
}
