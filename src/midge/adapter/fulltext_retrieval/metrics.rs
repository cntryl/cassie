//! Counters for full-text index write cost, kept apart from source-row bytes.

use std::sync::atomic::{AtomicU64, Ordering};

/// Index records written by one maintenance operation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FulltextWriteTally {
    /// Documents whose rows changed in the commit.
    pub(crate) documents: usize,
    /// Index records put (posting blocks, term records, statistics, metadata).
    pub(crate) pieces_written: usize,
    /// Key plus value bytes of every index record put.
    pub(crate) bytes_written: usize,
    /// Index records deleted.
    pub(crate) pieces_deleted: usize,
    /// Distinct terms whose postings were rewritten.
    pub(crate) terms_touched: usize,
    /// Posting blocks put.
    pub(crate) blocks_written: usize,
    /// True when the operation rebuilt the whole index instead of maintaining it.
    pub(crate) full_rebuild: bool,
}

fn add(counter: &AtomicU64, value: usize) {
    counter.fetch_add(u64::try_from(value).unwrap_or(u64::MAX), Ordering::Relaxed);
}

fn set(counter: &AtomicU64, value: usize) {
    counter.store(u64::try_from(value).unwrap_or(u64::MAX), Ordering::Relaxed);
}

#[derive(Debug, Default)]
pub(crate) struct FulltextMaintenanceMetrics {
    operations: AtomicU64,
    documents: AtomicU64,
    pieces_written: AtomicU64,
    bytes_written: AtomicU64,
    pieces_deleted: AtomicU64,
    terms_touched: AtomicU64,
    blocks_written: AtomicU64,
    last_documents: AtomicU64,
    last_pieces_written: AtomicU64,
    last_bytes_written: AtomicU64,
    last_pieces_deleted: AtomicU64,
    rebuilds: AtomicU64,
    rebuild_pieces_written: AtomicU64,
    rebuild_bytes_written: AtomicU64,
    rebuild_pieces_deleted: AtomicU64,
}

impl FulltextMaintenanceMetrics {
    /// Records one committed maintenance operation.
    pub(crate) fn record(&self, tally: &FulltextWriteTally) {
        if tally.full_rebuild {
            self.rebuilds.fetch_add(1, Ordering::Relaxed);
            add(&self.rebuild_pieces_written, tally.pieces_written);
            add(&self.rebuild_bytes_written, tally.bytes_written);
            add(&self.rebuild_pieces_deleted, tally.pieces_deleted);
            return;
        }
        self.operations.fetch_add(1, Ordering::Relaxed);
        add(&self.documents, tally.documents);
        add(&self.pieces_written, tally.pieces_written);
        add(&self.bytes_written, tally.bytes_written);
        add(&self.pieces_deleted, tally.pieces_deleted);
        add(&self.terms_touched, tally.terms_touched);
        add(&self.blocks_written, tally.blocks_written);
        set(&self.last_documents, tally.documents);
        set(&self.last_pieces_written, tally.pieces_written);
        set(&self.last_bytes_written, tally.bytes_written);
        set(&self.last_pieces_deleted, tally.pieces_deleted);
    }

    /// Counters merged into the `search` section of the runtime metrics.
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        serde_json::json!({
            "fulltext_maintenance_operations_total": load(&self.operations),
            "fulltext_maintenance_documents_total": load(&self.documents),
            "fulltext_maintenance_index_pieces_written_total": load(&self.pieces_written),
            "fulltext_maintenance_index_bytes_written_total": load(&self.bytes_written),
            "fulltext_maintenance_index_pieces_deleted_total": load(&self.pieces_deleted),
            "fulltext_maintenance_terms_touched_total": load(&self.terms_touched),
            "fulltext_maintenance_posting_blocks_written_total": load(&self.blocks_written),
            "fulltext_maintenance_last_documents": load(&self.last_documents),
            "fulltext_maintenance_last_index_pieces_written": load(&self.last_pieces_written),
            "fulltext_maintenance_last_index_bytes_written": load(&self.last_bytes_written),
            "fulltext_maintenance_last_index_pieces_deleted": load(&self.last_pieces_deleted),
            "fulltext_rebuilds_total": load(&self.rebuilds),
            "fulltext_rebuild_index_pieces_written_total": load(&self.rebuild_pieces_written),
            "fulltext_rebuild_index_bytes_written_total": load(&self.rebuild_bytes_written),
            "fulltext_rebuild_index_pieces_deleted_total": load(&self.rebuild_pieces_deleted),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_separate_incremental_and_rebuild_counters() {
        // Arrange
        let metrics = FulltextMaintenanceMetrics::default();
        let incremental = FulltextWriteTally {
            documents: 1,
            pieces_written: 4,
            bytes_written: 120,
            pieces_deleted: 1,
            terms_touched: 2,
            blocks_written: 2,
            full_rebuild: false,
        };
        let rebuild = FulltextWriteTally {
            pieces_written: 900,
            full_rebuild: true,
            ..FulltextWriteTally::default()
        };

        // Act
        metrics.record(&incremental);
        metrics.record(&rebuild);
        let snapshot = metrics.snapshot();

        // Assert
        assert_eq!(snapshot["fulltext_maintenance_operations_total"], 1);
        assert_eq!(
            snapshot["fulltext_maintenance_last_index_bytes_written"],
            120
        );
        assert_eq!(snapshot["fulltext_rebuilds_total"], 1);
        assert_eq!(snapshot["fulltext_rebuild_index_pieces_written_total"], 900);
    }
}
