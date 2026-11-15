//! Bounded native observations for durability benchmarking.

use super::commit_source::{current_commit_source, CommitSource};
use super::{CassieError, Midge};
#[cfg(feature = "replay-benchmark-observation")]
use std::cell::Cell;
use std::time::Duration;

/// Engine-wide counters; these do not attribute I/O to an individual replay.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct NativeDurabilityMetrics {
    pub wal_append_count: u64,
    pub wal_fsync_count: u64,
    pub wal_ledger_file_sync_attempts: u64,
    pub wal_ledger_file_syncs: u64,
    pub wal_ledger_directory_sync_attempts: u64,
    pub wal_ledger_directory_syncs: u64,
    pub wal_accounted_syncs: u64,
    pub current_sequence: u64,
    pub wal_local_durable_seq: u64,
}

impl Midge {
    /// Read the published Midge engine's durability counters without a write.
    /// Accounted syncs omit segment initialization, repair, and other filesystem syncs.
    ///
    /// # Errors
    ///
    /// Returns the native metrics error or timeout.
    pub fn native_durability_metrics(&self) -> Result<NativeDurabilityMetrics, CassieError> {
        let runtime = self
            .engine
            .metrics()
            .get_runtime_metrics_with_timeout(Duration::from_secs(5))
            .map_err(CassieError::from)?;
        Ok(NativeDurabilityMetrics {
            wal_append_count: runtime.wal_append_count,
            wal_fsync_count: runtime.wal_fsync_count,
            wal_ledger_file_sync_attempts: runtime.wal_ledger_file_sync_attempts,
            wal_ledger_file_syncs: runtime.wal_ledger_file_syncs,
            wal_ledger_directory_sync_attempts: runtime.wal_ledger_directory_sync_attempts,
            wal_ledger_directory_syncs: runtime.wal_ledger_directory_syncs,
            wal_accounted_syncs: runtime.wal_accounted_syncs,
            current_sequence: runtime.current_sequence,
            wal_local_durable_seq: runtime.wal_local_durable_seq,
        })
    }
}

#[cfg(feature = "replay-benchmark-observation")]
/// Native commit calls on the observing thread. Per-policy counters count
/// attempts, including failed or outcome-unknown calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct NativeCommitObservation {
    pub attempted: u64,
    pub first_policy: Option<&'static str>,
    pub succeeded: u64,
    pub failed: u64,
    pub outcome_unknown: u64,
    pub local_sync_attempts: u64,
    pub local_buffered_attempts: u64,
    pub cloud_async_attempts: u64,
    pub cloud_strict_attempts: u64,
    pub best_effort_attempts: u64,
    /// Attempts outside any post-write maintenance scope, such as the primary
    /// row and checkpoint commit.
    pub primary_attempts: u64,
    pub column_maintenance_attempts: u64,
    pub hash_maintenance_attempts: u64,
    pub time_series_maintenance_attempts: u64,
    /// Local-sync or cloud-strict attempts made inside a maintenance scope.
    pub maintenance_barrier_attempts: u64,
}

#[cfg(feature = "replay-benchmark-observation")]
thread_local! {
    static COMMITS: Cell<Option<NativeCommitObservation>> = const { Cell::new(None) };
}

#[cfg(feature = "replay-benchmark-observation")]
struct ObservationGuard;

#[cfg(feature = "replay-benchmark-observation")]
impl Drop for ObservationGuard {
    fn drop(&mut self) {
        COMMITS.set(None);
    }
}

/// Observe native commit calls made on this thread while `operation` runs.
/// Background threads, suppressed commits, and staged overlays are excluded.
/// The observed result is the native call result, before later Cassie work.
///
/// # Panics
///
/// Panics if observations are nested on the same thread, or if `operation` panics.
#[cfg(feature = "replay-benchmark-observation")]
#[doc(hidden)]
pub fn observe_native_commits<T>(operation: impl FnOnce() -> T) -> (T, NativeCommitObservation) {
    COMMITS.with(|commits| {
        assert!(
            commits.get().is_none(),
            "native commit observation cannot be nested"
        );
        commits.set(Some(NativeCommitObservation::default()));
    });
    let _guard = ObservationGuard;
    let result = operation();
    let observed = COMMITS.get().expect("active native commit observation");
    (result, observed)
}

#[cfg(feature = "replay-benchmark-observation")]
pub(super) fn observe_native_commit<T>(
    options: cntryl_midge::WriteOptions,
    operation: impl FnOnce() -> Result<T, cntryl_midge::MidgeError>,
) -> Result<T, cntryl_midge::MidgeError> {
    COMMITS.with(|commits| {
        if let Some(mut observed) = commits.get() {
            observed.attempted = observed.attempted.saturating_add(1);
            let (name, count) = match options.policy() {
                cntryl_midge::DurabilityPolicy::Sync => {
                    ("local_sync", &mut observed.local_sync_attempts)
                }
                cntryl_midge::DurabilityPolicy::Buffered => {
                    ("local_buffered", &mut observed.local_buffered_attempts)
                }
                cntryl_midge::DurabilityPolicy::CloudAsync => {
                    ("cloud_async", &mut observed.cloud_async_attempts)
                }
                cntryl_midge::DurabilityPolicy::CloudStrict => {
                    ("cloud_strict", &mut observed.cloud_strict_attempts)
                }
                cntryl_midge::DurabilityPolicy::BestEffort => {
                    ("best_effort", &mut observed.best_effort_attempts)
                }
            };
            *count = count.saturating_add(1);
            let source = current_commit_source();
            let source_count = match source {
                CommitSource::Primary => &mut observed.primary_attempts,
                CommitSource::ColumnMaintenance => &mut observed.column_maintenance_attempts,
                CommitSource::HashMaintenance => &mut observed.hash_maintenance_attempts,
                CommitSource::TimeSeriesMaintenance => {
                    &mut observed.time_series_maintenance_attempts
                }
            };
            *source_count = source_count.saturating_add(1);
            if source != CommitSource::Primary
                && matches!(
                    options.policy(),
                    cntryl_midge::DurabilityPolicy::Sync
                        | cntryl_midge::DurabilityPolicy::CloudStrict
                )
            {
                observed.maintenance_barrier_attempts =
                    observed.maintenance_barrier_attempts.saturating_add(1);
            }
            if observed.first_policy.is_none() {
                observed.first_policy = Some(name);
            }
            commits.set(Some(observed));
        }
    });
    let result = operation();
    COMMITS.with(|commits| {
        if let Some(mut observed) = commits.get() {
            match &result {
                Ok(_) => observed.succeeded = observed.succeeded.saturating_add(1),
                Err(cntryl_midge::MidgeError::OutcomeUnknown(_)) => {
                    observed.outcome_unknown = observed.outcome_unknown.saturating_add(1);
                }
                Err(_) => observed.failed = observed.failed.saturating_add(1),
            }
            commits.set(Some(observed));
        }
    });
    result
}

#[cfg(test)]
mod tests;
