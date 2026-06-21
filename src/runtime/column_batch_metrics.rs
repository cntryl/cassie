use super::RuntimeState;

impl RuntimeState {
    pub fn record_column_batch_scan(
        &self,
        rows: usize,
        compressed_bytes: usize,
        uncompressed_bytes: usize,
        skipped_segments: usize,
        decoded_columns: usize,
    ) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.scans += 1;
        metrics.column_batches.row_fetches_avoided += rows as u64;
        metrics.column_batches.compressed_bytes_total += compressed_bytes as u64;
        metrics.column_batches.uncompressed_bytes_total += uncompressed_bytes as u64;
        metrics.column_batches.skipped_segments += skipped_segments as u64;
        metrics.column_batches.decoded_columns += decoded_columns as u64;
    }

    pub fn record_column_batch_fallback(&self, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.last_fallback_reason = reason.to_string();
    }

    pub fn record_column_batch_decode_fallback(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.decode_fallbacks += 1;
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.last_fallback_reason = "decode".to_string();
    }

    pub fn record_column_batch_row_blob_fallback(&self, rows: usize, reason: &str) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.column_batches.fallback_scans += 1;
        metrics.column_batches.row_blob_fetches += rows as u64;
        metrics.column_batches.last_fallback_reason = reason.to_string();
    }
}
