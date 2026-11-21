//! Scheduled retention driving and exact row accounting for the mixed workload.

use super::{
    model, Cassie, CassieError, CassieSession, Fixture, Instant, MaintenanceResult, Mutation,
    Value, RETENTION_POLICY, TABLE,
};

/// Retention state read from `pg_retention_policies`.
struct RetentionProgress {
    pending: bool,
    pending_scanned_rows: u64,
    last_deleted_rows: u64,
}

/// Drives explicit retention batches until the pending cutoff clears.
///
/// Progress is exact, not timed: a batch scans a fixed number of rows in id
/// order, so every batch that leaves the cutoff pending must advance the
/// scanned-row counter. A batch that does not is an engine defect and aborts
/// the run instead of looping.
pub(super) fn enforce_retention(cassie: &Cassie, session: &CassieSession) -> MaintenanceResult {
    let started = Instant::now();
    let mut completed_batches = 0_u64;
    let mut scanned_before = 0_u64;
    loop {
        cassie
            .execute_sql(
                session,
                &format!(
                    "ENFORCE RETENTION POLICY {RETENTION_POLICY} DUE AT '{}'",
                    model::RETENTION_AT
                ),
                Vec::new(),
            )
            .expect("execute one scheduled retention batch");
        completed_batches = completed_batches.saturating_add(1);
        let progress = retention_progress(cassie, session);
        if !progress.pending {
            return MaintenanceResult {
                name: "retention_enforcement".to_owned(),
                elapsed: started.elapsed(),
                completed_batches,
                completed: true,
                deleted_rows: progress.last_deleted_rows,
            };
        }
        assert!(
            progress.pending_scanned_rows > scanned_before,
            "retention batch {completed_batches} left the cutoff pending without scanning new rows"
        );
        scanned_before = progress.pending_scanned_rows;
    }
}

pub(super) fn retention_pending(fixture: &Fixture) -> Result<bool, CassieError> {
    let result = fixture.execute(&retention_progress_sql())?;
    Ok(progress_from_rows(&result).pending)
}

fn retention_progress(cassie: &Cassie, session: &CassieSession) -> RetentionProgress {
    let result = cassie
        .execute_sql(session, &retention_progress_sql(), Vec::new())
        .expect("read retention progress");
    progress_from_rows(result.rows())
}

/// The catalog stores the policy under its database-qualified name.
fn retention_progress_sql() -> String {
    format!(
        "SELECT pending_cutoff, pending_scanned_rows, last_deleted_rows \
         FROM pg_catalog.pg_retention_policies WHERE policy_name LIKE '%.{RETENTION_POLICY}'"
    )
}

/// A missing policy row must never read as "retention finished".
fn progress_from_rows(rows: &[Vec<Value>]) -> RetentionProgress {
    let row = rows
        .first()
        .expect("the retention policy is listed in pg_retention_policies");
    let count = |value: &Value| {
        u64::try_from(value.as_i64().unwrap_or_default()).expect("retention counters are unsigned")
    };
    RetentionProgress {
        pending: !row[0].is_null(),
        pending_scanned_rows: count(&row[1]),
        last_deleted_rows: count(&row[2]),
    }
}

pub(super) struct RetentionAccounting {
    pub(super) expected_deleted: u64,
    pub(super) expired_remaining: u64,
    pub(super) mismatches: usize,
}

/// Exact retention row accounting against the event model.
///
/// Rows older than the retention boundary only come from the seed and the
/// backdated corrections, all of which are committed before retention starts, so
/// the number of expired rows in the model is exactly what retention must
/// delete. After retention no expired row may remain and the policy must report
/// that exact count; before it, every expired row must still be present.
pub(super) fn retention_accounting(
    fixture: &Fixture,
    committed: &[Mutation],
    maximum_position: u64,
    retention_applied: bool,
    reported_deleted: u64,
) -> RetentionAccounting {
    let unretained = model::apply_through_checkpoint(
        fixture.initial_rows.as_ref(),
        committed,
        maximum_position,
        false,
    );
    let expected_deleted = u64::try_from(
        unretained
            .values()
            .filter(|row| row.event_at.as_str() < model::RETENTION_BOUNDARY)
            .count(),
    )
    .expect("expired row count fits u64");
    let remaining = fixture
        .execute(&format!(
            "SELECT COUNT(*) FROM {TABLE} WHERE event_at < '{}'",
            model::RETENTION_BOUNDARY
        ))
        .expect("count expired rows");
    let expired_remaining = u64::try_from(remaining[0][0].as_i64().unwrap_or_default())
        .expect("expired row count is unsigned");
    let mismatches = if retention_applied {
        usize::from(expired_remaining != 0) + usize::from(reported_deleted != expected_deleted)
    } else {
        usize::from(expired_remaining != expected_deleted)
    };
    if mismatches > 0 {
        eprintln!(
            "mixed: retention accounting mismatch applied={retention_applied} expected_deleted={expected_deleted} reported_deleted={reported_deleted} expired_remaining={expired_remaining}"
        );
    }
    RetentionAccounting {
        expected_deleted,
        expired_remaining,
        mismatches,
    }
}
