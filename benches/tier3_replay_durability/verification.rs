//! Untimed checks after measurement and after clean reopen.

use super::mode::ReplayMode;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

#[path = "bucket.rs"]
mod bucket;

fn query(cassie: &Cassie, session: &CassieSession, sql: &str) -> cassie::executor::QueryResult {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| {
            panic!(
                "verification query failed: {sql}; SQLSTATE {}: {error}",
                error.sql_state()
            )
        })
}

fn scalar(cassie: &Cassie, session: &CassieSession, sql: &str) -> i64 {
    let result = query(cassie, session, sql);
    let rows = result.rows();
    assert_eq!(rows.len(), 1, "verification returns one row");
    match rows[0].first() {
        Some(Value::Int64(value)) => *value,
        value => panic!("expected integer verification scalar, got {value:?}"),
    }
}

pub(super) fn verify(cassie: &Cassie, mode: ReplayMode, total_events: usize) {
    let session = cassie.create_session("verification", None);
    let total = i64::try_from(total_events).expect("verification count fits i64");
    assert_eq!(
        scalar(cassie, &session, "SELECT COUNT(*) FROM replay_bench"),
        total
    );
    // Equality reads validate every title bucket through the selected execution
    // path. Indexed fixtures carry the scalar index; no forced index is claimed.
    for bucket in 0..16 {
        let expected = total / 16 + i64::from(bucket > 0 && bucket <= total % 16);
        let title = format!("title-{bucket}");
        let sql = format!("SELECT title FROM replay_bench WHERE title = '{title}'");
        let result = query(cassie, &session, &sql);
        bucket::assert_bucket(result.rows(), usize::try_from(expected).unwrap(), &title);
    }
    if mode.is_sql() {
        assert_eq!(
            scalar(
                cassie,
                &session,
                "SELECT position FROM replay_bench_checkpoint"
            ),
            total
        );
    } else {
        let projection = cassie.midge.canonical_collection_name("replay_bench");
        let metadata = cassie
            .midge
            .projection_metadata(&projection)
            .expect("projection metadata")
            .expect("stored projection");
        assert_eq!(
            metadata.source_checkpoint,
            Some(format!("checkpoint-{total_events}"))
        );
        assert_eq!(
            metadata.source_position,
            Some(u64::try_from(total_events).unwrap())
        );
        assert_eq!(
            metadata.applied_event_count,
            u64::try_from(total_events).unwrap()
        );
    }
}

/// Measures clean-open startup with the exact same selected fixture profile.
pub(super) fn clean_reopen(
    path: &std::path::Path,
    config: cassie::config::CassieRuntimeConfig,
    mode: ReplayMode,
    total_events: usize,
) -> u128 {
    let recovery_start = std::time::Instant::now();
    let recovered =
        Cassie::new_local_with_data_dir_and_config(path, config).expect("reopen local Cassie");
    recovered.startup().expect("reopened startup");
    let recovery_ns = recovery_start.elapsed().as_nanos();
    verify(&recovered, mode, total_events);
    recovered.shutdown();
    recovery_ns
}
