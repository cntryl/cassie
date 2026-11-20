//! Counters for vector index rebuild cost, kept apart from source-row bytes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

/// Rebuild cost counters. The `last_*` values describe the most recently
/// finished rebuild; concurrent rebuilds of different collections may blend
/// them, while the totals stay exact.
#[derive(Debug, Default)]
pub(crate) struct VectorIndexMaintenanceMetrics {
    rebuilds: AtomicU64,
    rebuild_micros_total: AtomicU64,
    last_rebuild_micros: AtomicU64,
    sidecar_transactions: AtomicU64,
    sidecar_bytes_written: AtomicU64,
    last_rebuild_bytes_written: AtomicU64,
    last_rebuild_transactions: AtomicU64,
    max_transaction_bytes: AtomicU64,
}

/// Counter positions captured when a rebuild starts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RebuildStart {
    bytes: u64,
    transactions: u64,
}

impl VectorIndexMaintenanceMetrics {
    /// Records one committed sidecar transaction and its estimated write bytes.
    pub(crate) fn record_transaction(&self, bytes: usize) {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        add(&self.sidecar_transactions, 1);
        add(&self.sidecar_bytes_written, bytes);
        self.max_transaction_bytes
            .fetch_max(bytes, Ordering::Relaxed);
    }

    /// Marks the counter positions a rebuild started from.
    pub(crate) fn begin_rebuild(&self) -> RebuildStart {
        RebuildStart {
            bytes: self.sidecar_bytes_written.load(Ordering::Relaxed),
            transactions: self.sidecar_transactions.load(Ordering::Relaxed),
        }
    }

    /// Records one completed rebuild started with [`Self::begin_rebuild`].
    pub(crate) fn finish_rebuild(&self, start: RebuildStart, elapsed: Duration) {
        let elapsed = micros(elapsed);
        add(&self.rebuilds, 1);
        add(&self.rebuild_micros_total, elapsed);
        self.last_rebuild_micros.store(elapsed, Ordering::Relaxed);
        self.last_rebuild_bytes_written.store(
            self.sidecar_bytes_written
                .load(Ordering::Relaxed)
                .saturating_sub(start.bytes),
            Ordering::Relaxed,
        );
        self.last_rebuild_transactions.store(
            self.sidecar_transactions
                .load(Ordering::Relaxed)
                .saturating_sub(start.transactions),
            Ordering::Relaxed,
        );
    }

    /// Counters merged into the `vector` section of the runtime metrics.
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        serde_json::json!({
            "index_rebuilds_total": load(&self.rebuilds),
            "index_rebuild_micros_total": load(&self.rebuild_micros_total),
            "index_last_rebuild_micros": load(&self.last_rebuild_micros),
            "index_sidecar_transactions_total": load(&self.sidecar_transactions),
            "index_sidecar_bytes_written_total": load(&self.sidecar_bytes_written),
            "index_last_rebuild_bytes_written": load(&self.last_rebuild_bytes_written),
            "index_last_rebuild_transactions": load(&self.last_rebuild_transactions),
            "index_max_sidecar_transaction_bytes": load(&self.max_transaction_bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_bytes_written_by_the_latest_rebuild_only() {
        // Arrange
        let metrics = VectorIndexMaintenanceMetrics::default();
        let first = metrics.begin_rebuild();
        metrics.record_transaction(100);
        metrics.finish_rebuild(first, Duration::from_micros(7));
        let second = metrics.begin_rebuild();
        metrics.record_transaction(40);
        metrics.record_transaction(60);

        // Act
        metrics.finish_rebuild(second, Duration::from_micros(9));
        let snapshot = metrics.snapshot();

        // Assert
        assert_eq!(snapshot["index_rebuilds_total"], 2);
        assert_eq!(snapshot["index_last_rebuild_bytes_written"], 100);
        assert_eq!(snapshot["index_last_rebuild_transactions"], 2);
        assert_eq!(snapshot["index_sidecar_bytes_written_total"], 200);
        assert_eq!(snapshot["index_max_sidecar_transaction_bytes"], 100);
        assert_eq!(snapshot["index_last_rebuild_micros"], 9);
    }
}
