//! Counters of the authoritative column vector reader (#1358).
//!
//! They are process-lifetime totals. A test or EXPLAIN ANALYZE differences two
//! snapshots, so concurrent unrelated scans must not run in the window.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Work done by [`super::ColumnVectorScan`] readers and by the legacy
/// row-document readers of column tables.
#[derive(Debug, Default)]
pub(crate) struct ColumnVectorReadCounters {
    scans: AtomicU64,
    batches: AtomicU64,
    lanes: AtomicU64,
    segments_pruned: AtomicU64,
    segments_untrusted: AtomicU64,
    values_fetched: AtomicU64,
    values_decoded: AtomicU64,
    row_documents_built: AtomicU64,
    staged_rows_merged: AtomicU64,
    disabled: AtomicBool,
}

impl ColumnVectorReadCounters {
    pub(crate) fn enabled(&self) -> bool {
        !self.disabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.disabled.store(!enabled, Ordering::Relaxed);
    }

    pub(crate) fn record_scan(&self) {
        Self::add(&self.scans, 1);
    }

    pub(crate) fn record_batch(&self, rows: usize, fields: usize) {
        Self::add(&self.batches, 1);
        Self::add(&self.lanes, rows.saturating_mul(fields) as u64);
    }

    pub(crate) fn record_segments(&self, pruned: u64, untrusted: u64) {
        Self::add(&self.segments_pruned, pruned);
        Self::add(&self.segments_untrusted, untrusted);
    }

    /// One stored field payload was fetched by the vector reader.
    pub(crate) fn record_value_fetched(&self) {
        Self::add(&self.values_fetched, 1);
    }

    /// One fetched payload was decoded into a lane cell.
    pub(crate) fn record_value_decoded(&self) {
        Self::add(&self.values_decoded, 1);
    }

    /// A row document (JSON object) was built from column storage.
    pub(crate) fn record_row_document(&self) {
        Self::add(&self.row_documents_built, 1);
    }

    /// A staged transaction row replaced or joined a persisted lane.
    pub(crate) fn record_staged_row(&self) {
        Self::add(&self.staged_rows_merged, 1);
    }

    pub(crate) fn snapshot(&self) -> ColumnVectorReadSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        ColumnVectorReadSnapshot {
            scans: load(&self.scans),
            batches: load(&self.batches),
            lanes: load(&self.lanes),
            segments_pruned: load(&self.segments_pruned),
            segments_untrusted: load(&self.segments_untrusted),
            values_fetched: load(&self.values_fetched),
            values_decoded: load(&self.values_decoded),
            row_documents_built: load(&self.row_documents_built),
            staged_rows_merged: load(&self.staged_rows_merged),
        }
    }

    fn add(counter: &AtomicU64, amount: u64) {
        counter.fetch_add(amount, Ordering::Relaxed);
    }
}

/// A point-in-time copy of the vector reader counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnVectorReadSnapshot {
    /// Vector scans opened.
    pub scans: u64,
    /// Lane batches returned.
    pub batches: u64,
    /// Selected-field cells returned (rows times selected fields).
    pub lanes: u64,
    /// Segments skipped because a verified summary excludes the filter.
    pub segments_pruned: u64,
    /// Segments whose summary was excluded but could not be trusted.
    pub segments_untrusted: u64,
    /// Stored field payloads fetched.
    pub values_fetched: u64,
    /// Fetched payloads decoded into lane cells.
    pub values_decoded: u64,
    /// Row documents built from column storage by any reader.
    pub row_documents_built: u64,
    /// Staged transaction rows merged into vector batches.
    pub staged_rows_merged: u64,
}

impl ColumnVectorReadSnapshot {
    /// The work done since `earlier`.
    #[must_use]
    pub fn since(self, earlier: Self) -> Self {
        Self {
            scans: self.scans.saturating_sub(earlier.scans),
            batches: self.batches.saturating_sub(earlier.batches),
            lanes: self.lanes.saturating_sub(earlier.lanes),
            segments_pruned: self.segments_pruned.saturating_sub(earlier.segments_pruned),
            segments_untrusted: self
                .segments_untrusted
                .saturating_sub(earlier.segments_untrusted),
            values_fetched: self.values_fetched.saturating_sub(earlier.values_fetched),
            values_decoded: self.values_decoded.saturating_sub(earlier.values_decoded),
            row_documents_built: self
                .row_documents_built
                .saturating_sub(earlier.row_documents_built),
            staged_rows_merged: self
                .staged_rows_merged
                .saturating_sub(earlier.staged_rows_merged),
        }
    }
}
