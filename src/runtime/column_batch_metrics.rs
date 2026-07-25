use super::RuntimeState;

pub(crate) struct ColumnBatchScanMetrics {
    pub(crate) rows: usize,
    pub(crate) segments_read: usize,
    pub(crate) chunks_read: usize,
    pub(crate) physical_bytes: usize,
    pub(crate) logical_bytes: usize,
    pub(crate) predicate_values: usize,
    pub(crate) candidate_rows: usize,
    pub(crate) selected_rows: usize,
    pub(crate) materialized_values: usize,
    pub(crate) skipped_segments: usize,
}

impl RuntimeState {
    pub(crate) fn record_column_batch_aggregate_scan(&self, selected_rows: usize) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.selected_rows = metrics
            .column_batches
            .selected_rows
            .saturating_add(selected_rows as u64);
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub(crate) fn record_column_batch_scan(&self, scan: &ColumnBatchScanMetrics) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.scans += 1;
        metrics.column_batches.row_fetches_avoided += scan.rows as u64;
        metrics.column_batches.segments_read += scan.segments_read as u64;
        metrics.column_batches.chunks_read += scan.chunks_read as u64;
        metrics.column_batches.physical_bytes_total += scan.physical_bytes as u64;
        metrics.column_batches.logical_bytes_total += scan.logical_bytes as u64;
        metrics.column_batches.predicate_values += scan.predicate_values as u64;
        let density_bucket = selection_density_bucket(scan.selected_rows, scan.candidate_rows);
        metrics.column_batches.selection_density_buckets[density_bucket] =
            metrics.column_batches.selection_density_buckets[density_bucket].saturating_add(1);
        metrics.column_batches.selected_rows += scan.selected_rows as u64;
        metrics.column_batches.materialized_values += scan.materialized_values as u64;
        metrics.column_batches.skipped_segments += scan.skipped_segments as u64;
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_column_batch_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.last_fallback_reason = reason.to_string();
    }

    pub fn record_column_batch_decode_fallback(&self) {
        self.record_column_batch_decode_fallback_with_reason("decode");
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_column_batch_decode_fallback_with_reason(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.decode_fallbacks += 1;
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.last_fallback_reason = reason.to_string();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_column_batch_row_blob_fallback(&self, rows: usize, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.row_blob_fetches += rows as u64;
        metrics.column_batches.last_fallback_reason = reason.to_string();
    }
}

fn selection_density_bucket(selected_rows: usize, candidate_rows: usize) -> usize {
    if selected_rows == 0 || candidate_rows == 0 {
        0
    } else if selected_rows.saturating_mul(10) <= candidate_rows {
        1
    } else if selected_rows.saturating_mul(2) <= candidate_rows {
        2
    } else if selected_rows < candidate_rows {
        3
    } else {
        4
    }
}
