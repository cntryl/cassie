//! Session-scoped I/O/WAL statistics and authorized SQL backend signaling.

use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;

use crate::support_backend_sessions::{assert_rows, with_sessions, Frames, Session, Sessions};
use crate::support_pgwire as wire;

async fn assert_backend_ids(sessions: &mut Sessions) {
    let admin = sessions.admin.query("SELECT pg_backend_pid()").await;
    let target = sessions.target.query("SELECT pg_backend_pid()").await;
    assert_rows(&admin, &[&[&sessions.admin.pid.to_string()]]);
    assert_rows(&target, &[&[&sessions.target.pid.to_string()]]);
    assert_ne!(sessions.admin.pid, sessions.target.pid);
}

async fn wait_for_query(sessions: &mut Sessions, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let sql = format!(
        "SELECT state, query FROM pg_stat_activity WHERE pid = {}",
        sessions.target.pid
    );
    loop {
        let frames = sessions.admin.query(&sql).await;
        assert_eq!(wire::error_code(&frames), None);
        if wire::data_rows(&frames).iter().any(|row| {
            row[0].as_deref() == Some("active")
                && row[1]
                    .as_deref()
                    .is_some_and(|query| query.contains(marker))
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "target query must become observable before signaling it"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wal_counts(session: &mut Session, pid: i32) -> (u64, u64) {
    let frames = session
        .query(&format!(
            "SELECT wal_records::text, wal_bytes::text FROM pg_stat_get_backend_wal({pid})"
        ))
        .await;
    assert_eq!(wire::error_code(&frames), None);
    let rows = wire::data_rows(&frames);
    assert_eq!(rows.len(), 1);
    let values = &rows[0];
    (
        values[0]
            .as_deref()
            .expect("WAL records")
            .parse()
            .expect("nonnegative WAL records"),
        values[1]
            .as_deref()
            .expect("WAL bytes")
            .parse()
            .expect("nonnegative WAL bytes"),
    )
}

async fn relation_io_operations(session: &mut Session, pid: i32) -> u64 {
    let frames = session
        .query(&format!(
            "SELECT COALESCE(sum(COALESCE(reads, 0) + COALESCE(writes, 0) + COALESCE(extends, 0) + COALESCE(hits, 0)), 0)::text FROM pg_stat_get_backend_io({pid}) \
             WHERE object = 'relation' AND context = 'normal'"
        ))
        .await;
    assert_eq!(wire::error_code(&frames), None);
    let rows = wire::data_rows(&frames);
    assert_eq!(rows.len(), 1);
    rows[0][0]
        .as_deref()
        .expect("relation I/O operation count")
        .parse()
        .expect("nonnegative relation I/O operation count")
}

async fn wait_for_wal(sessions: &mut Sessions, pid: i32, before: (u64, u64)) -> Frames {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let clear = sessions
            .admin
            .query("SELECT pg_stat_clear_snapshot()")
            .await;
        assert_eq!(wire::error_code(&clear), None);
        let frames = sessions
            .admin
            .query(&format!(
                "SELECT wal_records > {}, wal_bytes > {} FROM pg_stat_get_backend_wal({pid})",
                before.0, before.1
            ))
            .await;
        assert_eq!(wire::error_code(&frames), None);
        if wire::data_rows(&frames) == vec![vec![Some("t".to_owned()), Some("t".to_owned())]] {
            return frames;
        }
        assert!(
            Instant::now() < deadline,
            "the target's committed writes must become visible in its own WAL counters"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn should_scope_backend_statistics_given_multiple_sessions_when_io_and_wal_are_queried() {
    // Arrange
    with_sessions("backend-statistics-scope", async |sessions| {
        assert_backend_ids(sessions).await;
        let create = sessions
            .admin
            .query("CREATE TABLE backend_stats_rows (k integer)")
            .await;
        assert_rows(&create, &[]);
        let flush = sessions
            .admin
            .query("SELECT pg_stat_force_next_flush()")
            .await;
        assert_eq!(wire::error_code(&flush), None);
        let target_pid = sessions.target.pid;
        let admin_pid = sessions.admin.pid;
        let before = wal_counts(&mut sessions.admin, target_pid).await;
        let io_before = relation_io_operations(&mut sessions.admin, target_pid).await;
        let observer_before = wal_counts(&mut sessions.admin, admin_pid).await;

        // Act
        let insert = sessions
            .target
            .query("INSERT INTO backend_stats_rows VALUES (1), (2), (3)")
            .await;
        assert_rows(&insert, &[]);
        let flush = sessions
            .target
            .query("SELECT pg_stat_force_next_flush()")
            .await;
        assert_eq!(wire::error_code(&flush), None);
        let observed = wait_for_wal(sessions, target_pid, before).await;
        let observer_after = wal_counts(&mut sessions.admin, admin_pid).await;
        let io_after = relation_io_operations(&mut sessions.admin, target_pid).await;
        let io = sessions.admin.query(&format!("SELECT count(*) > 0, bool_and(backend_type = 'client backend'), bool_and(COALESCE(reads, 0) >= 0 AND COALESCE(writes, 0) >= 0) FROM pg_stat_get_backend_io({target_pid})")).await;
        let absent = sessions.admin.query("SELECT (SELECT count(*) FROM pg_stat_get_backend_io(-1)), (pg_stat_get_backend_wal(-1)).wal_records IS NULL").await;

        // Assert
        assert_rows(&observed, &[&["t", "t"]]);
        assert!(
            io_after > io_before,
            "inserting into the empty relation must increase the target backend's own I/O counters"
        );
        assert_eq!(
            observer_after, observer_before,
            "target writes must not increment the observing backend's WAL counters"
        );
        assert_rows(&io, &[&["t", "t", "t"]]);
        assert_rows(&absent, &[&["0", "t"]]);
    });
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

#[test]
fn should_cancel_the_target_query_given_an_authorized_session_when_pg_cancel_backend_is_called() {
    // Arrange
    with_sessions("sql-cancel-backend", async |sessions| {
        assert_backend_ids(sessions).await;
        let sleep_control = sessions.target.query("SELECT pg_sleep(0)").await;
        assert_eq!(wire::error_code(&sleep_control), None);
        sessions
            .target
            .send("SELECT pg_sleep(30) /* catalog_sql_cancel */")
            .await;
        wait_for_query(sessions, "catalog_sql_cancel").await;

        // Act
        let signaled = sessions
            .admin
            .query(&format!(
                "SELECT pg_cancel_backend({})",
                sessions.target.pid
            ))
            .await;
        assert_rows(&signaled, &[&["t"]]);
        let canceled = sessions.target.read().await;
        let recovered = sessions.target.query("SELECT 43").await;

        // Assert
        assert_rows(&signaled, &[&["t"]]);
        assert_eq!(wire::error_code(&canceled).as_deref(), Some("57014"));
        assert_eq!(
            wire::data_rows(&canceled),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(canceled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(
            canceled.last().map(|(_, body)| body.as_slice()),
            Some(&b"I"[..])
        );
        assert_rows(&recovered, &[&["43"]]);
    });
}

#[test]
fn should_terminate_the_target_session_given_an_authorized_session_when_pg_terminate_backend_is_called(
) {
    // Arrange
    with_sessions("sql-terminate-backend", async |sessions| {
        assert_backend_ids(sessions).await;
        let create = sessions
            .admin
            .query("CREATE TABLE terminated_backend_rows (k integer)")
            .await;
        assert_rows(&create, &[]);
        for sql in ["BEGIN", "INSERT INTO terminated_backend_rows VALUES (1)"] {
            let frames = sessions.target.query(sql).await;
            assert_rows(&frames, &[]);
        }

        // Act
        let signaled = sessions
            .admin
            .query(&format!(
                "SELECT pg_terminate_backend({}, 1000)",
                sessions.target.pid
            ))
            .await;
        assert_rows(&signaled, &[&["t"]]);
        let fatal = tokio::time::timeout(
            Duration::from_secs(3),
            wire::read_wire_frame(&mut sessions.target.reader),
        )
        .await
        .expect("termination ErrorResponse");
        let mut byte = [0_u8];
        let eof = tokio::time::timeout(
            Duration::from_secs(3),
            sessions.target.reader.read(&mut byte),
        )
        .await
        .expect("termination closes the socket")
        .expect("read terminal socket state");
        let remaining = sessions
            .admin
            .query("SELECT count(*)::bigint FROM terminated_backend_rows")
            .await;
        let activity = sessions
            .admin
            .query(&format!(
                "SELECT count(*)::bigint FROM pg_stat_activity WHERE pid = {}",
                sessions.target.pid
            ))
            .await;

        // Assert
        assert_rows(&signaled, &[&["t"]]);
        assert_eq!(fatal.0, b'E');
        assert!(wire::parse_error_fields(&fatal.1).contains(&('C', "57P01".to_owned())));
        assert_eq!(eof, 0);
        assert_rows(&remaining, &[&["0"]]);
        assert_rows(&activity, &[&["0"]]);
    });
}
