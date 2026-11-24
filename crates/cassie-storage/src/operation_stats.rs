//! Counters returned by concrete storage operations.

#[derive(Default)]
pub struct ColumnBatchScanMetrics {
    pub rows: usize,
    pub segments_read: usize,
    pub chunks_read: usize,
    pub physical_bytes: usize,
    pub logical_bytes: usize,
    pub predicate_values: usize,
    pub candidate_rows: usize,
    pub selected_rows: usize,
    pub materialized_values: usize,
    pub skipped_segments: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectionWriteStats {
    pub row_puts: u64,
    pub row_deletes: u64,
    pub index_puts: u64,
    pub index_deletes: u64,
    pub metadata_puts: u64,
    pub metadata_deletes: u64,
    pub duplicate_checks: u64,
    pub batch_flushes: u64,
    pub rebuild_target_puts: u64,
    pub activation_metadata_writes: u64,
}
